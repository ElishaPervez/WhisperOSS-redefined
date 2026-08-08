//! The dictation loop (spec §5): hook events → hold tracker → record →
//! silence gate → transcribe (stale results discarded by generation) →
//! privacy paste → sequenced clipboard restore. One dictation at a time;
//! a new hold makes any in-flight result stale. Errors are logged only
//! in M1 — the overlay error state arrives in M2.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use crate::{applog, audio, clipboard, dsp, groq, hook, hotkey_logic};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PASTE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

fn set_overlay_visible(app: &tauri::AppHandle, visible: bool) {
    if let Some(w) = app.get_webview_window("overlay") {
        if visible {
            // Re-anchor to the monitor the cursor is on right now.
            let _ = crate::position_overlay(app);
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    }
}

pub fn start(app: tauri::AppHandle, audio: Arc<audio::AudioEngine>, api_key: String) {
    let (tx, rx) = channel();
    hook::spawn(tx);

    let client = Arc::new(groq::GroqClient::new(
        api_key,
        groq::PROD_BASE_URL.to_string(),
        REQUEST_TIMEOUT,
    ));
    let generation = Arc::new(AtomicU64::new(0));

    std::thread::spawn(move || {
        let mut tracker = hotkey_logic::HoldTracker::new();
        for ev in rx {
            match tracker.on_event(ev) {
                hotkey_logic::Action::None => {}
                hotkey_logic::Action::Start => {
                    generation.fetch_add(1, Ordering::SeqCst);
                    if !audio.is_healthy() {
                        applog::log("recording-refused-no-mic");
                        continue;
                    }
                    audio.start_recording();
                    set_overlay_visible(&app, true);
                    applog::log("recording-start");
                }
                hotkey_logic::Action::Cancel => {
                    let _ = audio.stop_recording();
                    set_overlay_visible(&app, false);
                    applog::log("recording-cancel-short-tap");
                }
                hotkey_logic::Action::Finish { held_ms } => {
                    let (samples, rate) = audio.stop_recording();
                    applog::log(&format!(
                        "recording-finish held_ms={held_ms} samples={}",
                        samples.len()
                    ));
                    if dsp::is_effectively_silent(&samples) {
                        set_overlay_visible(&app, false);
                        applog::log("silent-discarded");
                        continue;
                    }
                    let wav = dsp::encode_wav_mono16(
                        &dsp::resample_to_16k(&samples, rate),
                        16_000,
                    );
                    let my_gen = generation.load(Ordering::SeqCst);
                    let client = client.clone();
                    let generation = generation.clone();
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let result = client.transcribe(wav);
                        let stale = generation.load(Ordering::SeqCst) != my_gen;
                        match result {
                            Ok(text) if stale => applog::log("result-discarded-stale"),
                            Ok(text) if text.is_empty() => applog::log("empty-transcript"),
                            Ok(text) => paste(&text),
                            Err(e) => applog::log(&format!("transcribe-error {e:?}")),
                        }
                        set_overlay_visible(&app, false);
                    });
                }
            }
        }
    });
}

fn paste(text: &str) {
    let previous = clipboard::snapshot_text();
    if previous.is_none() {
        applog::log("clipboard-snapshot-empty-or-nontext");
    }
    if !clipboard::stage(text, previous) {
        // Privacy formats could not be set: NEVER paste unprotected (spec §6).
        applog::log("paste-aborted-privacy-staging-failed");
        return;
    }
    std::thread::sleep(Duration::from_millis(60)); // let the clipboard settle
    clipboard::send_ctrl_v();
    let confirmed = clipboard::wait_pasted(PASTE_CONFIRM_TIMEOUT);
    std::thread::sleep(Duration::from_millis(250));
    clipboard::restore();
    applog::log(if confirmed { "pasted-confirmed" } else { "paste-unconfirmed" });
}
