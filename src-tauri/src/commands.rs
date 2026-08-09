//! Tauri commands the settings window calls. Each mutates the shared state,
//! persists to config.json, and (where relevant) applies immediately. The
//! window never touches config.json directly — it goes through these.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{Emitter, Manager, State};

use crate::{applog, audio, autostart, config, groq, hook, hotkey_logic, keys, state::AppState};

/// Only three themes are valid; anything else falls back to "auto".
pub fn normalize_theme(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "dark" => "dark".into(),
        "light" => "light".into(),
        _ => "auto".into(),
    }
}

fn persist(state: &AppState) {
    let cfg = state.config.lock().unwrap();
    config::save(&cfg);
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> config::Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn has_api_key(state: State<AppState>) -> bool {
    !state.key.lock().unwrap().is_empty()
}

#[tauri::command]
pub fn set_formatter(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().use_formatter = value;
    persist(&state);
    applog::log("setting-formatter-changed");
}

#[tauri::command]
pub fn set_casual(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().casual_mode = value;
    persist(&state);
    applog::log("setting-casual-changed");
}

#[tauri::command]
pub fn set_theme(state: State<AppState>, value: String) {
    state.config.lock().unwrap().theme = normalize_theme(&value);
    persist(&state);
    applog::log("setting-theme-changed");
}

#[tauri::command]
pub fn set_autostart(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().run_on_startup = value;
    persist(&state);
    autostart::reconcile(value);
}

/// Validate the key against Groq; on success persist it to Credential Manager
/// and update the live key so the next dictation uses it. Returns Ok(()) or
/// a short error message for inline display.
#[tauri::command]
pub fn save_api_key(state: State<AppState>, key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Enter a key".into());
    }
    let client = groq::GroqClient::new(
        key.clone(),
        groq::PROD_BASE_URL.to_string(),
        std::time::Duration::from_secs(15),
    );
    match client.validate_key() {
        Ok(()) => {
            if !keys::save(&key) {
                return Err("Couldn't save to Credential Manager".into());
            }
            *state.key.lock().unwrap() = key;
            Ok(())
        }
        Err(groq::GroqError::Unauthorized) => Err("Groq rejected this key".into()),
        Err(groq::GroqError::Network(_)) => Err("Couldn't reach Groq".into()),
        Err(groq::GroqError::Server(_)) => Err("Groq error — try again".into()),
    }
}

#[tauri::command]
pub fn list_microphones() -> Vec<String> {
    audio::list_input_devices()
}

#[tauri::command]
pub fn set_microphone(state: State<AppState>, value: Option<String>) {
    let value = value.filter(|v| !v.trim().is_empty());
    state.config.lock().unwrap().input_device = value.clone();
    persist(&state);
    state.audio.switch_device(value);
    applog::log("setting-microphone-changed");
}

/// Start listening for a new combo. Everything typed anywhere on the machine
/// is swallowed until this ends, so it always ends: on the first key release,
/// on Escape, when the window loses focus, or on the watchdog below.
#[tauri::command]
pub fn begin_hotkey_capture(app: tauri::AppHandle, state: State<AppState>) {
    // A dictation must not survive into capture mode.
    state.generation.fetch_add(1, Ordering::SeqCst);
    let _ = state.audio.stop_recording();
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }

    let my_gen = state.capture_gen.load(Ordering::SeqCst);
    state.capturing.store(true, Ordering::SeqCst);
    hook::set_capture(true);
    applog::log(&format!(
        "hotkey-capture-begin hook_flag={}",
        hook::is_capturing()
    ));

    let capturing = state.capturing.clone();
    let capture_gen = state.capture_gen.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(hotkey_logic::CAPTURE_TIMEOUT_MS));
        if capture_gen.load(Ordering::SeqCst) == my_gen
            && capturing.swap(false, Ordering::SeqCst)
        {
            hook::set_capture(false);
            capture_gen.fetch_add(1, Ordering::SeqCst);
            applog::log("hotkey-capture-timeout");
            let _ = app.emit(
                "hotkey",
                serde_json::json!({ "phase": "cancelled", "keys": [] }),
            );
        }
    });
}

#[tauri::command]
pub fn cancel_hotkey_capture(app: tauri::AppHandle, state: State<AppState>, reason: String) {
    if state.capturing.swap(false, Ordering::SeqCst) {
        hook::set_capture(false);
        state.capture_gen.fetch_add(1, Ordering::SeqCst);
        applog::log(&format!("hotkey-capture-cancelled reason={reason}"));
        let _ = app.emit(
            "hotkey",
            serde_json::json!({ "phase": "cancelled", "keys": [] }),
        );
    } else {
        applog::log(&format!("hotkey-capture-cancel-ignored reason={reason}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_normalization() {
        assert_eq!(normalize_theme("dark"), "dark");
        assert_eq!(normalize_theme("light"), "light");
        assert_eq!(normalize_theme("auto"), "auto");
        assert_eq!(normalize_theme("AUTO"), "auto");
        assert_eq!(normalize_theme("nonsense"), "auto");
        assert_eq!(normalize_theme(""), "auto");
    }
}
