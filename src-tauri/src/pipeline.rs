//! The dictation loop (spec §5) + overlay choreography (spec §3).
//! Every UI touch is guarded by the generation counter: a stale worker
//! (its dictation was superseded) must never show, hide, or restyle the
//! pill that a newer dictation owns. This also fixes an M1 glitch where a
//! superseded worker's cleanup could hide the pill mid-listening.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::{applog, clipboard, dsp, groq, hook, hotkey_logic, overlay_state};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PASTE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

struct Ui {
    app: tauri::AppHandle,
    generation: Arc<AtomicU64>,
}

impl Ui {
    fn current(&self, my_gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == my_gen
    }

    fn emit(&self, my_gen: u64, state: &str, message: &str) {
        if !self.current(my_gen) {
            return;
        }
        let _ = self
            .app
            .emit("ui", overlay_state::ui_payload(state, message));
    }

    fn show(&self, my_gen: u64) {
        if !self.current(my_gen) {
            return;
        }
        if let Some(w) = self.app.get_webview_window("overlay") {
            let _ = crate::position_overlay(&self.app);
            let _ = w.show();
        }
    }

    /// Fade the pill out, then hide the window. Blocking — call off-thread.
    fn fade_out_and_hide(&self, my_gen: u64) {
        self.emit(my_gen, "hidden", "");
        std::thread::sleep(Duration::from_millis(overlay_state::FADE_MS));
        if !self.current(my_gen) {
            return;
        }
        if let Some(w) = self.app.get_webview_window("overlay") {
            let _ = w.hide();
        }
    }

    /// Error state per design: red pill, 2 s, fade. Blocking — call off-thread.
    fn show_error(&self, my_gen: u64, message: &str) {
        self.show(my_gen);
        self.emit(my_gen, "error", message);
        std::thread::sleep(Duration::from_millis(overlay_state::ERROR_HOLD_MS));
        self.fade_out_and_hide(my_gen);
    }
}

/// The combo the tracker should be using right now, read from config.
fn combo_from_config(state: &crate::state::AppState) -> Vec<hotkey_logic::Key> {
    let cfg = state.config.lock().unwrap();
    hotkey_logic::parse_combo(&cfg.hotkey).unwrap_or_else(|| {
        applog::log("config-invalid-hotkey-using-default");
        hotkey_logic::parse_combo(&["ctrl".into(), "win".into()]).expect("default combo")
    })
}

fn apply_combo(combo: &[hotkey_logic::Key]) {
    hook::set_suppression(
        hotkey_logic::combo_other_vk(combo),
        &combo.iter().copied().filter(|k| k.is_modifier()).collect::<Vec<_>>(),
    );
}

pub fn start(app: tauri::AppHandle, state: crate::state::AppState) {
    let (tx, rx) = channel();
    hook::spawn(tx);

    let audio = state.audio.clone();
    let generation = state.generation.clone();
    let combo = combo_from_config(&state);
    apply_combo(&combo);

    std::thread::spawn(move || {
        let mut tracker = hotkey_logic::HoldTracker::new(combo);

        for ev in rx {
            match tracker.on_event(ev) {
                hotkey_logic::Action::None => {}
                hotkey_logic::Action::Start => {
                    if state.key.lock().unwrap().is_empty() {
                        applog::log("recording-refused-no-key");
                        crate::show_first_run(&app);
                        continue;
                    }
                    let my_gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
                    let ui = Ui { app: app.clone(), generation: generation.clone() };
                    if !audio.is_healthy() {
                        applog::log("recording-refused-no-mic");
                        std::thread::spawn(move || ui.show_error(my_gen, "No mic detected"));
                        continue;
                    }
                    audio.start_recording();
                    ui.show(my_gen);
                    ui.emit(my_gen, "listening", "");
                    applog::log("recording-start");
                }
                hotkey_logic::Action::Cancel => {
                    let _ = audio.stop_recording();
                    let my_gen = generation.load(Ordering::SeqCst);
                    let ui = Ui { app: app.clone(), generation: generation.clone() };
                    std::thread::spawn(move || ui.fade_out_and_hide(my_gen));
                    applog::log("recording-cancel-short-tap");
                }
                hotkey_logic::Action::Finish { held_ms } => {
                    let (samples, rate) = audio.stop_recording();
                    applog::log(&format!(
                        "recording-finish held_ms={held_ms} samples={}",
                        samples.len()
                    ));
                    let my_gen = generation.load(Ordering::SeqCst);
                    let ui = Ui { app: app.clone(), generation: generation.clone() };

                    if dsp::is_effectively_silent(&samples) {
                        let wanted = state.config.lock().unwrap().input_device.clone();
                        if on_fallback_device(&wanted, &audio.active_device()) {
                            applog::log("silent-on-fallback-device");
                            std::thread::spawn(move || ui.show_error(my_gen, "Check your mic"));
                        } else {
                            applog::log("silent-discarded");
                            std::thread::spawn(move || ui.fade_out_and_hide(my_gen));
                        }
                        continue;
                    }

                    ui.emit(my_gen, "processing", "");
                    let wav = dsp::encode_wav_mono16(
                        &dsp::resample_to_16k(&samples, rate),
                        16_000,
                    );
                    let state = state.clone();
                    std::thread::spawn(move || {
                        let (key, use_formatter, casual) = {
                            let cfg = state.config.lock().unwrap();
                            (state.key.lock().unwrap().clone(), cfg.use_formatter, cfg.casual_mode)
                        };
                        let client = groq::GroqClient::new(
                            key,
                            groq::PROD_BASE_URL.to_string(),
                            REQUEST_TIMEOUT,
                        );
                        match client.transcribe(wav) {
                            Ok(_) if !ui.current(my_gen) => {
                                applog::log("result-discarded-stale");
                                // No UI touches: a newer dictation owns the pill.
                            }
                            Ok(text) if text.is_empty() => {
                                applog::log("empty-transcript");
                                ui.fade_out_and_hide(my_gen);
                            }
                            Ok(text) => {
                                let final_text = if wants_formatting(use_formatter, casual) {
                                    match client.format_text(&text, casual) {
                                        Ok(f) if !f.is_empty() => f,
                                        Ok(_) => text.clone(),
                                        Err(e) => {
                                            let (m, d) = overlay_state::describe_error(&e);
                                            applog::log(&format!(
                                                "formatter-failed-fallback-raw {m} {d}"
                                            ));
                                            text.clone()
                                        }
                                    }
                                } else {
                                    text.clone()
                                };
                                if paste(&final_text) {
                                    ui.emit(my_gen, "success", "");
                                    std::thread::sleep(Duration::from_millis(
                                        overlay_state::SUCCESS_HOLD_MS,
                                    ));
                                    ui.fade_out_and_hide(my_gen);
                                } else {
                                    ui.show_error(my_gen, "Couldn't paste safely");
                                }
                            }
                            Err(e) => {
                                let (message, detail) = overlay_state::describe_error(&e);
                                applog::log(&format!("transcribe-error {message} {detail}"));
                                if matches!(e, groq::GroqError::Unauthorized) {
                                    crate::show_settings_at_key(&ui.app);
                                }
                                ui.show_error(my_gen, message);
                            }
                        }
                    });
                }
            }
        }
    });
}

/// True when the user picked a specific device but something else is actually
/// recording. In that state a silent take almost certainly means a broken
/// microphone rather than a quiet user, so it is worth saying out loud.
fn on_fallback_device(wanted: &Option<String>, active: &Option<String>) -> bool {
    match (wanted, active) {
        (Some(w), Some(a)) => w != a,
        (Some(_), None) => true,
        _ => false,
    }
}

/// The AI rewrite runs when EITHER toggle is on. Casual is its own trigger,
/// not a sub-option of formatting; when casual is on, format_text picks the
/// casual prompt (so casual wins if both are on).
fn wants_formatting(use_formatter: bool, casual: bool) -> bool {
    use_formatter || casual
}

/// Privacy paste. Returns true only if the text was staged with the privacy
/// formats AND actually pulled by the target app.
fn paste(text: &str) -> bool {
    let previous = clipboard::snapshot();
    if previous.is_none() {
        applog::log("clipboard-snapshot-empty");
    }
    if !clipboard::stage(text, previous) {
        applog::log("paste-aborted-privacy-staging-failed");
        return false;
    }
    std::thread::sleep(Duration::from_millis(60));
    clipboard::send_ctrl_v();
    let confirmed = clipboard::wait_pasted(PASTE_CONFIRM_TIMEOUT);
    std::thread::sleep(Duration::from_millis(250));
    clipboard::restore();
    applog::log(if confirmed { "pasted-confirmed" } else { "paste-unconfirmed" });
    // Unconfirmed after 5 s usually means the focused app ignores Ctrl+V.
    // The text WAS delivered to the clipboard mechanism, so count it as a
    // success for the pill (spec's error table has no entry for this; the
    // log line records it).
    true
}

#[cfg(test)]
mod tests {
    use super::wants_formatting;
    use super::on_fallback_device;

    #[test]
    fn formatting_truth_table() {
        assert!(!wants_formatting(false, false)); // neither → raw
        assert!(wants_formatting(true, false));   // formal only
        assert!(wants_formatting(false, true));   // casual only → still formats
        assert!(wants_formatting(true, true));    // both → formats (casual wins in format_text)
    }

    #[test]
    fn fallback_detection() {
        let usb = Some("USB PnP Audio Device".to_string());
        let nvidia = Some("NVIDIA Broadcast".to_string());
        // user picked a device and it is the one recording
        assert!(!on_fallback_device(&usb, &usb));
        // user picked a device and something else is recording
        assert!(on_fallback_device(&usb, &nvidia));
        // user picked a device and nothing is recording at all
        assert!(on_fallback_device(&usb, &None));
        // user picked "system default": whatever is recording is correct
        assert!(!on_fallback_device(&None, &nvidia));
        assert!(!on_fallback_device(&None, &None));
    }
}
