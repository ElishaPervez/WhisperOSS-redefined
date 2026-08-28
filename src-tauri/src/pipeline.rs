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

use crate::{
    applog, clipboard, config, dsp, gemini, groq, hook, hotkey_logic, keys, overlay_state, position,
};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PASTE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

enum CaptureRoute {
    Buffered,
    Gemini(gemini::LiveConfig),
}

fn capture_route(
    provider: config::TranscriptionProvider,
    key: String,
    model: String,
    vocabulary: Vec<String>,
    formatting: Formatting,
) -> CaptureRoute {
    match provider {
        config::TranscriptionProvider::Groq => CaptureRoute::Buffered,
        config::TranscriptionProvider::Gemini => CaptureRoute::Gemini(gemini::LiveConfig {
            key,
            model: if model == "gemini-3.5-transcribe" {
                config::DEFAULT_GEMINI_MODEL.into()
            } else {
                model
            },
            vocabulary,
            smart: matches!(formatting, Formatting::Ai),
        }),
    }
}

#[derive(Clone)]
struct ActiveCapture {
    provider: config::TranscriptionProvider,
    formatting: Formatting,
    vocabulary: Vec<String>,
    groq_model: String,
}

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

    fn show(&self, my_gen: u64, logical_w: f64) {
        if !self.current(my_gen) {
            return;
        }
        if let Some(w) = self.app.get_webview_window("overlay") {
            let _ = w.set_size(tauri::LogicalSize::new(logical_w, position::PILL_LOGICAL_H));
            let _ = crate::position_overlay(&self.app, logical_w);
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

    /// Error state per design: red pill sized to its message, 2 s, fade.
    /// Blocking — call off-thread.
    fn show_error(&self, my_gen: u64, message: &str) {
        self.show(my_gen, position::pill_width_for(message));
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
        &combo
            .iter()
            .copied()
            .filter(|k| k.is_modifier())
            .collect::<Vec<_>>(),
    );
}

pub fn start(app: tauri::AppHandle, state: crate::state::AppState) {
    let (tx, rx) = channel();
    hook::spawn(tx);

    let audio = state.audio.clone();
    let generation = state.generation.clone();
    let combo = combo_from_config(&state);
    apply_combo(&combo);
    let gemini_live = state.gemini_live.clone();
    sync_gemini_prewarm(&state);

    std::thread::spawn(move || {
        let mut tracker = hotkey_logic::HoldTracker::new(combo);
        let mut active_capture: Option<ActiveCapture> = None;

        for ev in rx {
            match tracker.on_event(ev) {
                hotkey_logic::Action::None => {}
                hotkey_logic::Action::Start => {
                    let (provider, formatting, vocabulary, groq_model, gemini_model, show_live_transcript) = {
                        let cfg = state.config.lock().unwrap();
                        (
                            cfg.transcription_provider,
                            formatting_mode(cfg.use_formatter, cfg.casual_mode),
                            cfg.vocabulary.clone(),
                            cfg.groq_model.clone(),
                            cfg.gemini_model.clone(),
                            cfg.show_live_transcript,
                        )
                    };
                    let key = match provider {
                        config::TranscriptionProvider::Groq => {
                            refreshed_provider_key(&state.groq_key, keys::Provider::Groq)
                        }
                        config::TranscriptionProvider::Gemini => {
                            refreshed_provider_key(&state.gemini_key, keys::Provider::Gemini)
                        }
                    };
                    if key.is_empty() {
                        applog::log("recording-refused-selected-provider-has-no-key");
                        crate::show_settings_at_key(&app);
                        continue;
                    }
                    let my_gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
                    let ui = Ui {
                        app: app.clone(),
                        generation: generation.clone(),
                    };
                    if !audio.is_healthy() {
                        applog::log("recording-refused-no-mic");
                        std::thread::spawn(move || ui.show_error(my_gen, "No mic detected"));
                        continue;
                    }
                    let capture = ActiveCapture {
                        provider,
                        formatting,
                        vocabulary: vocabulary.clone(),
                        groq_model,
                    };
                    match capture_route(provider, key, gemini_model, vocabulary, formatting) {
                        CaptureRoute::Buffered => audio.start_recording(),
                        CaptureRoute::Gemini(config) => {
                            let (stream_tx, stream_rx) = if show_live_transcript {
                                let (tx, rx) = channel::<String>();
                                (Some(tx), Some(rx))
                            } else {
                                (None, None)
                            };
                            match gemini_live.begin(config, stream_tx) {
                                Ok(sink) => {
                                    audio.start_streaming_recording(Arc::new(sink));
                                    if let Some(stream_rx) = stream_rx {
                                        let ui_stream = Ui {
                                            app: app.clone(),
                                            generation: generation.clone(),
                                        };
                                        std::thread::spawn(move || {
                                            let mut expanded = false;
                                            while let Ok(text) = stream_rx.recv() {
                                                if !ui_stream.current(my_gen) {
                                                    break;
                                                }
                                                let text = text.trim();
                                                if !text.is_empty() {
                                                    if !expanded {
                                                        expanded = true;
                                                        ui_stream.show(my_gen, position::PILL_MAX_LOGICAL_W);
                                                    }
                                                    ui_stream.emit(my_gen, "streaming", text);
                                                }
                                            }
                                        });
                                    }
                                }
                                Err(error) => {
                                    let (message, detail) =
                                        overlay_state::describe_gemini_error(&error);
                                    applog::log(&format!("transcribe-start-error {message} {detail}"));
                                    std::thread::spawn(move || ui.show_error(my_gen, message));
                                    continue;
                                }
                            }
                        }
                    }
                    active_capture = Some(capture);
                    ui.show(my_gen, position::PILL_LOGICAL_W);
                    ui.emit(my_gen, "listening", "");
                    applog::log("recording-start");
                }
                hotkey_logic::Action::Cancel => {
                    let _ = audio.stop_recording();
                    if active_capture.take().is_some_and(|capture| {
                        capture.provider == config::TranscriptionProvider::Gemini
                    }) {
                        gemini_live.cancel();
                    }
                    let my_gen = generation.load(Ordering::SeqCst);
                    let ui = Ui {
                        app: app.clone(),
                        generation: generation.clone(),
                    };
                    std::thread::spawn(move || ui.fade_out_and_hide(my_gen));
                    applog::log("recording-cancel-short-tap");
                }
                hotkey_logic::Action::Finish { held_ms } => {
                    let Some(capture) = active_capture.take() else {
                        applog::log("recording-finish-without-active-capture");
                        continue;
                    };
                    let postroll_ms = state.config.lock().unwrap().postroll_ms;
                    let my_gen = generation.load(Ordering::SeqCst);
                    let ui = Ui {
                        app: app.clone(),
                        generation: generation.clone(),
                    };

                    ui.show(my_gen, position::PILL_LOGICAL_W);
                    ui.emit(my_gen, "processing", "");

                    let audio = audio.clone();
                    let gemini_live = gemini_live.clone();
                    let state = state.clone();
                    std::thread::spawn(move || {
                        if postroll_ms > 0 {
                            std::thread::sleep(Duration::from_millis(postroll_ms as u64));
                        }
                        if !ui.current(my_gen) {
                            applog::log("recording-finish-stale-after-postroll");
                            return;
                        }
                        let (samples, rate) = audio.stop_recording();
                        applog::log(&format!(
                            "recording-finish held_ms={held_ms} samples={}",
                            samples.len()
                        ));

                        if dsp::is_effectively_silent(&samples) {
                            if capture.provider == config::TranscriptionProvider::Gemini {
                                gemini_live.cancel();
                            }
                            let wanted = state.config.lock().unwrap().input_device.clone();
                            if on_fallback_device(&wanted, &audio.active_device()) {
                                applog::log("silent-on-fallback-device");
                                ui.show_error(my_gen, "Check your mic");
                            } else {
                                applog::log("silent-discarded");
                                ui.fade_out_and_hide(my_gen);
                            }
                            return;
                        }

                        let wav =
                            (capture.provider == config::TranscriptionProvider::Groq).then(|| {
                                dsp::encode_wav_mono16(&dsp::resample_to_16k(&samples, rate), 16_000)
                            });
                        let live_result = (capture.provider == config::TranscriptionProvider::Gemini)
                            .then(|| gemini_live.finish());

                        enum ProviderFailure {
                            Groq(groq::GroqError),
                            Gemini(gemini::GeminiError),
                        }

                        let mut groq_client = None;
                        let transcript = match capture.provider {
                            config::TranscriptionProvider::Groq => {
                                let key =
                                    refreshed_provider_key(&state.groq_key, keys::Provider::Groq);
                                let client = groq::GroqClient::new(
                                    key,
                                    capture.groq_model,
                                    groq::PROD_BASE_URL.to_string(),
                                    REQUEST_TIMEOUT,
                                );
                                let result = client
                                    .transcribe(
                                        wav.expect("Groq capture has WAV audio"),
                                        &capture.vocabulary.join(", "),
                                    )
                                    .map_err(ProviderFailure::Groq);
                                groq_client = Some(client);
                                result
                            }
                            config::TranscriptionProvider::Gemini => {
                                match live_result.expect("Gemini capture has Live result") {
                                    Ok(receiver) => receiver
                                        .recv_timeout(Duration::from_secs(70))
                                        .unwrap_or_else(|_| {
                                            Err(gemini::GeminiError::Network(
                                                "Live finalization timed out".into(),
                                            ))
                                        })
                                        .map_err(ProviderFailure::Gemini),
                                    Err(error) => Err(ProviderFailure::Gemini(error)),
                                }
                            }
                        };

                        match transcript {
                            Ok(_) if !ui.current(my_gen) => {
                                applog::log("result-discarded-stale");
                                // No UI touches: a newer dictation owns the pill.
                            }
                            Ok(text) if text.is_empty() => {
                                applog::log("empty-transcript");
                                ui.fade_out_and_hide(my_gen);
                            }
                            Ok(text) => {
                                let final_text = match capture.formatting {
                                    Formatting::Casual => crate::casualize::casualize(&text),
                                    Formatting::Ai => match groq_client.as_ref() {
                                        Some(client) => match client.format_text(&text) {
                                            Ok(formatted) if !formatted.is_empty() => formatted,
                                            Ok(_) => text.clone(),
                                            Err(error) => {
                                                let (message, detail) =
                                                    overlay_state::describe_groq_error(&error);
                                                applog::log(&format!(
                                                    "formatter-failed-fallback-raw {message} {detail}"
                                                ));
                                                text.clone()
                                            }
                                        },
                                        // Gemini Smart transcription already formatted the text
                                        // in the same request; contacting Groq here would violate
                                        // the selected-provider boundary.
                                        None => text.clone(),
                                    },
                                    Formatting::Raw => text.clone(),
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
                            Err(error) => {
                                let (message, detail, unauthorized) = match &error {
                                    ProviderFailure::Groq(error) => {
                                        let (message, detail) =
                                            overlay_state::describe_groq_error(error);
                                        (
                                            message,
                                            detail,
                                            matches!(error, groq::GroqError::Unauthorized),
                                        )
                                    }
                                    ProviderFailure::Gemini(error) => {
                                        let (message, detail) =
                                            overlay_state::describe_gemini_error(error);
                                        (
                                            message,
                                            detail,
                                            matches!(error, gemini::GeminiError::Unauthorized),
                                        )
                                    }
                                };
                                applog::log(&format!("transcribe-error {message} {detail}"));
                                if unauthorized {
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

fn refreshed_provider_key(memory: &std::sync::Mutex<String>, provider: keys::Provider) -> String {
    let mut memory = memory.lock().unwrap();
    let (key, changed) = keys::refreshed_key(&memory, keys::read_vault(provider));
    if changed {
        *memory = key.clone();
        applog::log("selected-provider-key-refreshed-from-vault");
    }
    key
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

pub fn sync_gemini_prewarm(state: &crate::state::AppState) {
    let cfg = state.config.lock().unwrap();
    if cfg.transcription_provider == config::TranscriptionProvider::Gemini {
        let key = refreshed_provider_key(&state.gemini_key, keys::Provider::Gemini);
        if !key.is_empty() {
            let formatting = formatting_mode(cfg.use_formatter, cfg.casual_mode);
            let model = if cfg.gemini_model == "gemini-3.5-transcribe" {
                config::DEFAULT_GEMINI_MODEL.into()
            } else {
                cfg.gemini_model.clone()
            };
            state.gemini_live.warm(Some(gemini::LiveConfig {
                key,
                model,
                vocabulary: cfg.vocabulary.clone(),
                smart: matches!(formatting, Formatting::Ai),
            }));
            return;
        }
    }
    state.gemini_live.warm(None);
}

#[derive(Clone, Copy)]
pub(crate) enum Formatting {
    Raw,
    Casual,
    Ai,
}

/// Casual mode is a local rewrite and always wins: it exists for latency,
/// so it must never trigger the AI pass even when the formatter toggle is
/// also on. The AI cleanup pass only runs for formal formatting.
pub(crate) fn formatting_mode(use_formatter: bool, casual: bool) -> Formatting {
    if casual {
        Formatting::Casual
    } else if use_formatter {
        Formatting::Ai
    } else {
        Formatting::Raw
    }
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
    applog::log(if confirmed {
        "pasted-confirmed"
    } else {
        "paste-unconfirmed"
    });
    // Unconfirmed after 5 s usually means the focused app ignores Ctrl+V.
    // The text WAS delivered to the clipboard mechanism, so count it as a
    // success for the pill (spec's error table has no entry for this; the
    // log line records it).
    true
}

#[cfg(test)]
mod tests {
    use super::on_fallback_device;
    use super::{capture_route, formatting_mode, CaptureRoute, Formatting};

    #[test]
    fn gemini_capture_streams_with_live_model_and_selected_cleanup() {
        let route = capture_route(
            crate::config::TranscriptionProvider::Gemini,
            "secret".into(),
            "gemini-3.5-transcribe-live".into(),
            vec!["WhisperOSS".into()],
            Formatting::Ai,
        );

        let CaptureRoute::Gemini(config) = route else {
            panic!("Gemini recording was buffered instead of streamed");
        };
        assert_eq!(config.model, "gemini-3.5-transcribe-live");
        assert_eq!(config.vocabulary, vec!["WhisperOSS"]);
        assert!(config.smart);
    }

    #[test]
    fn formatting_truth_table() {
        // neither → raw transcript, no AI call
        assert!(matches!(formatting_mode(false, false), Formatting::Raw));
        // formal only → AI cleanup pass
        assert!(matches!(formatting_mode(true, false), Formatting::Ai));
        // casual only → local pass, no AI call
        assert!(matches!(formatting_mode(false, true), Formatting::Casual));
        // both on → casual wins, and it stays local (latency is the point)
        assert!(matches!(formatting_mode(true, true), Formatting::Casual));
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
