//! Always-on microphone capture (spec §4). The stream never stops: it feeds
//! a configurable pre-roll ring so recording start is instant and the first word
//! is never clipped. Mono i16 at the device's native rate; downsampling to
//! 16 kHz happens at upload time (dsp.rs). The device is swappable at runtime
//! (M3c) — see switch_device.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::Emitter;

use crate::{applog, dsp};

/// Names of all available input devices, for the settings picker (M3b).
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else { return Vec::new() };
    devices.filter_map(|d| d.name().ok()).collect()
}

fn pick_device(preferred: &Option<String>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = preferred {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.name().ok().as_deref() == Some(name)) {
                return Some(d);
            }
        }
        crate::applog::log("audio-preferred-device-missing-using-default");
    }
    host.default_input_device()
}

pub trait AudioStreamSink: Send + Sync {
    fn push(&self, samples: Vec<i16>, source_rate: u32);
}

struct RecordingBuffer {
    samples: Vec<i16>,
    sink: Option<Arc<dyn AudioStreamSink>>,
}

impl RecordingBuffer {
    fn start(
        samples: Vec<i16>,
        source_rate: u32,
        sink: Option<Arc<dyn AudioStreamSink>>,
    ) -> Self {
        if let Some(stream) = sink.as_ref() {
            stream.push(samples.clone(), source_rate);
        }
        Self { samples, sink }
    }

    fn append(&mut self, samples: &[i16], source_rate: u32) {
        self.samples.extend_from_slice(samples);
        if let Some(stream) = self.sink.as_ref() {
            stream.push(samples.to_vec(), source_rate);
        }
    }
}

pub struct AudioEngine {
    /// Kept so the engine can tell the settings window when the microphone
    /// situation changes while that window is already open.
    app: tauri::AppHandle,
    ring: Mutex<VecDeque<i16>>,
    recording: Mutex<Option<RecordingBuffer>>,
    rate: AtomicU32,
    preroll_ms: AtomicU32,
    peak: AtomicU16,
    healthy: AtomicBool,
    /// The device the running stream actually opened — not necessarily the
    /// one the user picked, since a refused device falls back to the default.
    active_device: Mutex<Option<String>>,
    /// The picked device we last tried to move back to and could not open.
    /// Checked so a device that is listed but permanently refusing cannot
    /// cause a reopen attempt every two seconds.
    refused_reclaim: Mutex<Option<String>>,
    /// Carries the next device name to the stream thread. The stream object
    /// itself can only exist on that thread.
    device_tx: Mutex<Sender<Option<String>>>,
}

impl AudioEngine {
    pub fn start(
        app: tauri::AppHandle,
        preferred: Option<String>,
        preroll_ms: u32,
    ) -> Arc<AudioEngine> {
        let app_for_engine = app.clone();
        let (device_tx, device_rx) = channel::<Option<String>>();
        let engine = Arc::new(AudioEngine {
            app: app_for_engine,
            ring: Mutex::new(VecDeque::new()),
            recording: Mutex::new(None),
            rate: AtomicU32::new(16_000),
            preroll_ms: AtomicU32::new(preroll_ms),
            peak: AtomicU16::new(0),
            healthy: AtomicBool::new(false),
            active_device: Mutex::new(None),
            refused_reclaim: Mutex::new(None),
            device_tx: Mutex::new(device_tx),
        });

        // The cpal stream is not Send: build it, hold it, and drop it all on
        // this one thread. It blocks here until a device change arrives.
        let e = engine.clone();
        std::thread::spawn(move || {
            use std::sync::mpsc::RecvTimeoutError;
            let mut wanted = preferred;
            let mut stream = open(&e, &wanted, false);
            loop {
                match device_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(next) => {
                        // Dropping on this thread is required: cpal streams
                        // are !Send.
                        drop(stream.take());
                        e.reset_buffers();
                        wanted = next;
                        stream = open(&e, &wanted, true);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // Nobody asked for a change. If there is no working
                        // stream — no mic at boot, one unplugged, or a device
                        // Windows refused — try again.
                        if !e.is_healthy() {
                            drop(stream.take());
                            e.reset_buffers();
                            stream = reopen_quietly(&e, &wanted);
                        } else {
                            // Healthy, but possibly on a fallback. If the
                            // device the user picked is back, move to it.
                            match e.reclaim_check(&wanted) {
                                Reclaim::Attempt => {
                                    applog::log("audio-reclaiming-preferred-device");
                                    drop(stream.take());
                                    e.reset_buffers();
                                    stream = open(&e, &wanted, true);
                                    if e.active_device().as_deref() != wanted.as_deref() {
                                        // Listed but would not open: remember,
                                        // or this repeats every two seconds.
                                        *e.refused_reclaim.lock().unwrap() = wanted.clone();
                                    }
                                }
                                Reclaim::ForgetRefusal => {
                                    *e.refused_reclaim.lock().unwrap() = None;
                                }
                                Reclaim::Stay => {}
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
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

    pub fn set_preroll_ms(&self, ms: u32) {
        self.preroll_ms.store(ms, Ordering::SeqCst);
        let rate = self.rate.load(Ordering::SeqCst);
        let ring_cap = preroll_samples(ms, rate);
        let mut ring = self.ring.lock().unwrap();
        while ring.len() > ring_cap {
            ring.pop_front();
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    pub fn active_device(&self) -> Option<String> {
        self.active_device.lock().unwrap().clone()
    }

    /// Only called on real transitions — lost, recovered, switched — so an
    /// already-open settings window refreshes without the two-second retry
    /// loop firing an event every two seconds while a device stays broken.
    fn announce_change(&self) {
        let _ = self.app.emit("mic-changed", ());
    }

    /// The two-second tick's decision about moving back to the picked device.
    /// The steady state — already on the picked device, or no pick at all —
    /// returns before enumerating devices, so the tick stays free.
    fn reclaim_check(&self, wanted: &Option<String>) -> Reclaim {
        let Some(name) = wanted.as_deref() else { return Reclaim::Stay };
        let active = self.active_device();
        if active.as_deref() == Some(name) {
            return Reclaim::Stay;
        }
        let recording = self.recording.lock().unwrap().is_some();
        let listed = list_input_devices().iter().any(|d| d == name);
        let refused = self.refused_reclaim.lock().unwrap().clone();
        reclaim_step(Some(name), active.as_deref(), recording, listed, refused.as_deref())
    }

    /// Change the capture device without restarting the app. The pre-roll
    /// buffer is thrown away because the new device may run at a different
    /// sample rate, and splicing the two would garble the first half-second.
    pub fn switch_device(&self, preferred: Option<String>) {
        applog::log("audio-switch-device-requested");
        *self.refused_reclaim.lock().unwrap() = None;
        let _ = self.device_tx.lock().unwrap().send(preferred);
    }

    fn reset_buffers(&self) {
        self.ring.lock().unwrap().clear();
        *self.recording.lock().unwrap() = None;
    }

    /// Instant start: seed the take with the pre-roll ring contents.
    pub fn start_recording(&self) {
        self.start_recording_with_sink(None);
    }

    pub fn start_streaming_recording(&self, sink: Arc<dyn AudioStreamSink>) {
        self.start_recording_with_sink(Some(sink));
    }

    fn start_recording_with_sink(&self, sink: Option<Arc<dyn AudioStreamSink>>) {
        let ring = self.ring.lock().unwrap();
        let seed: Vec<i16> = ring.iter().copied().collect();
        let rate = self.rate.load(Ordering::SeqCst);
        *self.recording.lock().unwrap() = Some(RecordingBuffer::start(seed, rate, sink));
        drop(ring);
    }

    pub fn stop_recording(&self) -> (Vec<i16>, u32) {
        let samples = self
            .recording
            .lock()
            .unwrap()
            .take()
            .map(|recording| recording.samples)
            .unwrap_or_default();
        (samples, self.rate.load(Ordering::SeqCst))
    }

    fn ingest(&self, mono: &[i16]) {
        let preroll_ms = self.preroll_ms.load(Ordering::SeqCst);
        let rate = self.rate.load(Ordering::SeqCst);
        let ring_cap = preroll_samples(preroll_ms, rate);
        {
            let mut ring = self.ring.lock().unwrap();
            if ring_cap == 0 {
                ring.clear();
            } else {
                for &s in mono {
                    if ring.len() >= ring_cap {
                        ring.pop_front();
                    }
                    ring.push_back(s);
                }
            }
        }
        if let Some(recording) = self.recording.lock().unwrap().as_mut() {
            recording.append(mono, rate);
        }
        let peak = mono.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        self.peak.fetch_max(peak, Ordering::SeqCst);
    }
}

pub fn preroll_samples(preroll_ms: u32, rate: u32) -> usize {
    (rate as f64 * (preroll_ms as f64 / 1000.0)).round() as usize
}

/// What the two-second tick should do about getting back to the device the
/// user actually picked. Pure so it can be tested.
#[derive(Debug, PartialEq)]
enum Reclaim {
    /// Nothing to do — no picked device, on the right one already, mid-take,
    /// or the device is listed but refused to open last time.
    Stay,
    /// The picked device vanished from Windows again: forget a remembered
    /// refusal so its next appearance gets a fresh attempt.
    ForgetRefusal,
    /// The picked device is back in Windows — move to it.
    Attempt,
}

fn reclaim_step(
    wanted: Option<&str>,
    active: Option<&str>,
    recording: bool,
    listed: bool,
    refused: Option<&str>,
) -> Reclaim {
    let Some(name) = wanted else { return Reclaim::Stay };
    if active == Some(name) {
        return Reclaim::Stay;
    }
    if !listed {
        return Reclaim::ForgetRefusal;
    }
    if recording || refused == Some(name) {
        return Reclaim::Stay;
    }
    Reclaim::Attempt
}

/// Try the user's chosen device, then the Windows default if it refuses.
/// `pick_device` already handles a device whose *name* has vanished; this
/// handles one that is still listed but will not open — held by another app,
/// or offering a format we cannot use. Without this the retry loop would
/// reopen the same broken device every two seconds forever.
fn build_with_fallback(
    engine: &Arc<AudioEngine>,
    preferred: &Option<String>,
    log_failure: bool,
) -> Result<cpal::Stream, String> {
    match build_stream(engine, preferred) {
        Ok(s) => Ok(s),
        Err(first) if preferred.is_some() => {
            if log_failure {
                applog::log(&format!("audio-preferred-device-refused {first}"));
            }
            build_stream(engine, &None)
        }
        Err(e) => Err(e),
    }
}

/// Build and start a stream, keeping the healthy flag honest. A dictation
/// attempted while this is None gets the "No mic detected" pill.
fn open(
    engine: &Arc<AudioEngine>,
    preferred: &Option<String>,
    announce: bool,
) -> Option<cpal::Stream> {
    engine.healthy.store(false, Ordering::SeqCst);
    match build_with_fallback(engine, preferred, true) {
        Ok(stream) => {
            if stream.play().is_ok() {
                engine.healthy.store(true, Ordering::SeqCst);
                applog::log("audio-stream-started");
                if announce {
                    engine.announce_change();
                }
                Some(stream)
            } else {
                applog::log("audio-stream-play-failed");
                *engine.active_device.lock().unwrap() = None;
                if announce {
                    engine.announce_change();
                }
                None
            }
        }
        Err(msg) => {
            applog::log(&format!("audio-stream-error {msg}"));
            *engine.active_device.lock().unwrap() = None;
            if announce {
                engine.announce_change();
            }
            None
        }
    }
}

/// The retry path, run every two seconds while there is no working stream.
/// It stays silent while it keeps failing — otherwise a machine with no
/// microphone would write a log line every two seconds forever — and writes
/// exactly one line when a device finally appears.
fn reopen_quietly(engine: &Arc<AudioEngine>, preferred: &Option<String>) -> Option<cpal::Stream> {
    engine.healthy.store(false, Ordering::SeqCst);
    let stream = match build_with_fallback(engine, preferred, false) {
        Ok(s) => s,
        Err(_) => {
            *engine.active_device.lock().unwrap() = None;
            return None;
        }
    };
    if stream.play().is_ok() {
        engine.healthy.store(true, Ordering::SeqCst);
        applog::log("audio-stream-recovered");
        engine.announce_change();
        Some(stream)
    } else {
        *engine.active_device.lock().unwrap() = None;
        None
    }
}

fn build_stream(
    engine: &Arc<AudioEngine>,
    preferred: &Option<String>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = pick_device(preferred).ok_or("no input device")?;
    *engine.active_device.lock().unwrap() = device.name().ok();
    let _ = host; // host only needed by pick_device/list; drop if warned
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    engine.rate.store(rate, Ordering::SeqCst);

    let e = engine.clone();
    // A device that dies mid-stream must stop counting as healthy, or the
    // retry loop below will never notice it needs to reopen.
    let e_err = engine.clone();
    let err_fn = move |err| {
        applog::log(&format!("audio-callback-error {err}"));
        e_err.healthy.store(false, Ordering::SeqCst);
        e_err.announce_change();
    };

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
                    e.ingest(&mono);
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
                    e.ingest(&mono);
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        other => return Err(format!("unsupported sample format {other:?}")),
    };
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::{reclaim_step, Reclaim};
    use super::{AudioStreamSink, RecordingBuffer};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct PacketCollector(Mutex<Vec<(Vec<i16>, u32)>>);

    impl AudioStreamSink for PacketCollector {
        fn push(&self, samples: Vec<i16>, source_rate: u32) {
            self.0.lock().unwrap().push((samples, source_rate));
        }
    }

    #[test]
    fn recording_sends_preroll_then_live_chunks_without_losing_local_audio() {
        let collector = Arc::new(PacketCollector::default());
        let sink: Arc<dyn AudioStreamSink> = collector.clone();
        let mut recording = RecordingBuffer::start(vec![1, 2], 48_000, Some(sink));

        recording.append(&[3, 4], 48_000);

        assert_eq!(recording.samples, vec![1, 2, 3, 4]);
        assert_eq!(
            *collector.0.lock().unwrap(),
            vec![(vec![1, 2], 48_000), (vec![3, 4], 48_000)]
        );
    }

    #[test]
    fn reclaim_decision() {
        use Reclaim::*;
        let usb = Some("USB PnP Audio Device");
        let nvidia = Some("NVIDIA Broadcast");
        // user picked "system default": there is nothing to go back to
        assert_eq!(reclaim_step(None, nvidia, false, false, None), Stay);
        // already on the picked device
        assert_eq!(reclaim_step(usb, usb, false, true, None), Stay);
        // picked device still absent from Windows: wait, and forget any refusal
        assert_eq!(reclaim_step(usb, nvidia, false, false, None), ForgetRefusal);
        assert_eq!(reclaim_step(usb, nvidia, false, false, usb), ForgetRefusal);
        // picked device is back: move to it
        assert_eq!(reclaim_step(usb, nvidia, false, true, None), Attempt);
        assert_eq!(reclaim_step(usb, None, false, true, None), Attempt);
        // back, but a recording is running: not in the middle of a take
        assert_eq!(reclaim_step(usb, nvidia, true, true, None), Stay);
        // listed but it refused to open last time: do not churn every two seconds
        assert_eq!(reclaim_step(usb, nvidia, false, true, usb), Stay);
    }

    #[test]
    fn preroll_samples_calculation() {
        use super::preroll_samples;
        // 500ms at 16kHz
        assert_eq!(preroll_samples(500, 16_000), 8_000);
        // 250ms at 16kHz
        assert_eq!(preroll_samples(250, 16_000), 4_000);
        // 0ms at 16kHz
        assert_eq!(preroll_samples(0, 16_000), 0);
        // 100ms at 48kHz
        assert_eq!(preroll_samples(100, 48_000), 4_800);
    }
}
