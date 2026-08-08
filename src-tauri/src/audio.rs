//! Always-on microphone capture (spec §4). The stream never stops: it feeds
//! a 0.5 s pre-roll ring so recording start is instant and the first word
//! is never clipped. Mono i16 at the device's native rate; downsampling to
//! 16 kHz happens at upload time (dsp.rs). Default input device only in M1
//! (device picker is M3).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::Emitter;

use crate::{applog, dsp};

const PRE_ROLL_SECS: f64 = 0.5;

pub struct AudioEngine {
    ring: Mutex<VecDeque<i16>>,
    recording: Mutex<Option<Vec<i16>>>,
    rate: AtomicU32,
    peak: AtomicU16,
    healthy: AtomicBool,
}

impl AudioEngine {
    pub fn start(app: tauri::AppHandle) -> Arc<AudioEngine> {
        let engine = Arc::new(AudioEngine {
            ring: Mutex::new(VecDeque::new()),
            recording: Mutex::new(None),
            rate: AtomicU32::new(16_000),
            peak: AtomicU16::new(0),
            healthy: AtomicBool::new(false),
        });

        // The cpal stream is not Send: build it on a dedicated thread and
        // park that thread forever to keep the stream alive.
        let e = engine.clone();
        std::thread::spawn(move || {
            match build_stream(&e) {
                Ok(stream) => {
                    if stream.play().is_ok() {
                        e.healthy.store(true, Ordering::SeqCst);
                        applog::log("audio-stream-started");
                    } else {
                        applog::log("audio-stream-play-failed");
                    }
                    loop {
                        std::thread::park();
                    }
                }
                Err(msg) => applog::log(&format!("audio-stream-error {msg}")),
            }
        });

        // Level emitter for the visualizer: ~30 Hz while recording.
        let e = engine.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(33));
            if e.recording.lock().unwrap().is_some() {
                let p = e.peak.swap(0, Ordering::SeqCst);
                let _ = app.emit("level", dsp::normalize_level(p as i16));
            }
        });

        engine
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    /// Instant start: seed the take with the pre-roll ring contents.
    pub fn start_recording(&self) {
        let seed: Vec<i16> = self.ring.lock().unwrap().iter().copied().collect();
        *self.recording.lock().unwrap() = Some(seed);
    }

    pub fn stop_recording(&self) -> (Vec<i16>, u32) {
        let samples = self.recording.lock().unwrap().take().unwrap_or_default();
        (samples, self.rate.load(Ordering::SeqCst))
    }

    fn ingest(&self, mono: &[i16], ring_cap: usize) {
        {
            let mut ring = self.ring.lock().unwrap();
            for &s in mono {
                if ring.len() == ring_cap {
                    ring.pop_front();
                }
                ring.push_back(s);
            }
        }
        if let Some(buf) = self.recording.lock().unwrap().as_mut() {
            buf.extend_from_slice(mono);
        }
        let peak = mono.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        self.peak.fetch_max(peak, Ordering::SeqCst);
    }
}

fn build_stream(engine: &Arc<AudioEngine>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("no input device")?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    engine.rate.store(rate, Ordering::SeqCst);
    let ring_cap = (rate as f64 * PRE_ROLL_SECS) as usize;

    let e = engine.clone();
    let err_fn = |err| applog::log(&format!("audio-callback-error {err}"));

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels)
                        .map(|frame| {
                            let avg = frame.iter().sum::<f32>() / frame.len() as f32;
                            (avg.clamp(-1.0, 1.0) * 32_767.0) as i16
                        })
                        .collect();
                    e.ingest(&mono, ring_cap);
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels)
                        .map(|frame| {
                            (frame.iter().map(|&s| s as i32).sum::<i32>()
                                / frame.len() as i32) as i16
                        })
                        .collect();
                    e.ingest(&mono, ring_cap);
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        other => return Err(format!("unsupported sample format {other:?}")),
    };
    Ok(stream)
}
