//! Tauri commands the settings window calls. Each mutates the shared state,
//! persists to config.json, and (where relevant) applies immediately. The
//! window never touches config.json directly — it goes through these.

use tauri::{Manager, State};

use crate::{applog, audio, autostart, config, gemini, groq, keys, pipeline, state::AppState};

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

#[derive(serde::Serialize)]
pub struct KeyStatus {
    pub groq: bool,
    pub gemini: bool,
}

#[tauri::command]
pub fn get_key_status(state: State<AppState>) -> KeyStatus {
    KeyStatus {
        groq: !state.groq_key.lock().unwrap().is_empty(),
        gemini: !state.gemini_key.lock().unwrap().is_empty(),
    }
}

#[tauri::command]
pub fn has_api_key(state: State<AppState>) -> bool {
    let provider = state.config.lock().unwrap().transcription_provider;
    match provider {
        config::TranscriptionProvider::Groq => !state.groq_key.lock().unwrap().is_empty(),
        config::TranscriptionProvider::Gemini => !state.gemini_key.lock().unwrap().is_empty(),
    }
}

#[tauri::command]
pub fn set_transcription_provider(state: State<AppState>, value: String) {
    let provider = if value.eq_ignore_ascii_case("gemini") {
        config::TranscriptionProvider::Gemini
    } else {
        config::TranscriptionProvider::Groq
    };
    state.config.lock().unwrap().transcription_provider = provider;
    persist(&state);
    pipeline::sync_gemini_prewarm(&state);
    applog::log("setting-transcription-provider-changed");
}

fn sanitized_model(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'))
    {
        value.to_string()
    } else {
        fallback.to_string()
    }
}

#[tauri::command]
pub fn set_provider_model(state: State<AppState>, provider: String, value: String) {
    let mut cfg = state.config.lock().unwrap();
    if provider.eq_ignore_ascii_case("gemini") {
        cfg.gemini_model = sanitized_model(&value, config::DEFAULT_GEMINI_MODEL);
    } else {
        cfg.groq_model = sanitized_model(&value, config::DEFAULT_GROQ_MODEL);
    }
    drop(cfg);
    persist(&state);
    pipeline::sync_gemini_prewarm(&state);
    applog::log("setting-transcription-model-changed");
}

#[tauri::command]
pub fn set_formatter(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().use_formatter = value;
    persist(&state);
    pipeline::sync_gemini_prewarm(&state);
    applog::log("setting-formatter-changed");
}

#[tauri::command]
pub fn set_casual(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().casual_mode = value;
    persist(&state);
    pipeline::sync_gemini_prewarm(&state);
    applog::log("setting-casual-changed");
}

#[tauri::command]
pub fn set_show_live_transcript(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().show_live_transcript = value;
    persist(&state);
    applog::log("setting-show-live-transcript-changed");
}

#[tauri::command]
pub fn set_theme(state: State<AppState>, value: String) {
    state.config.lock().unwrap().theme = normalize_theme(&value);
    persist(&state);
    applog::log("setting-theme-changed");
}

fn sanitize_vocabulary(value: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut sanitized = Vec::new();

    for entry in value {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        let comparison_key = entry.to_lowercase();
        if seen.insert(comparison_key) {
            sanitized.push(entry.to_string());
        }
    }

    sanitized
}

#[tauri::command]
pub fn set_vocabulary(state: State<AppState>, value: Vec<String>) {
    state.config.lock().unwrap().vocabulary = sanitize_vocabulary(value);
    persist(&state);
    pipeline::sync_gemini_prewarm(&state);
    applog::log("setting-vocabulary-changed");
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
    save_key(&state, config::TranscriptionProvider::Groq, key)
}

#[tauri::command]
pub fn save_provider_key(
    state: State<AppState>,
    provider: String,
    key: String,
) -> Result<(), String> {
    let provider = if provider.eq_ignore_ascii_case("gemini") {
        config::TranscriptionProvider::Gemini
    } else {
        config::TranscriptionProvider::Groq
    };
    save_key(&state, provider, key)
}

fn save_key(
    state: &AppState,
    provider: config::TranscriptionProvider,
    key: String,
) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Enter a key".into());
    }

    let timeout = std::time::Duration::from_secs(15);
    match provider {
        config::TranscriptionProvider::Groq => {
            let model = state.config.lock().unwrap().groq_model.clone();
            let client =
                groq::GroqClient::new(key.clone(), model, groq::PROD_BASE_URL.to_string(), timeout);
            match client.validate_key() {
                Ok(()) => {
                    if !keys::save(keys::Provider::Groq, &key) {
                        return Err("Couldn't save to Credential Manager".into());
                    }
                    *state.groq_key.lock().unwrap() = key;
                    Ok(())
                }
                Err(groq::GroqError::Unauthorized) => Err("Groq rejected this key".into()),
                Err(groq::GroqError::Network(_)) => Err("Couldn't reach Groq".into()),
                Err(groq::GroqError::Server(_)) => Err("Groq error, try again".into()),
            }
        }
        config::TranscriptionProvider::Gemini => {
            let model = state.config.lock().unwrap().gemini_model.clone();
            let client = gemini::GeminiClient::new(
                key.clone(),
                model,
                gemini::PROD_BASE_URL.to_string(),
                timeout,
            );
            match client.validate_key() {
                Ok(()) => {
                    if !keys::save(keys::Provider::Gemini, &key) {
                        return Err("Couldn't save to Credential Manager".into());
                    }
                    *state.gemini_key.lock().unwrap() = key;
                    pipeline::sync_gemini_prewarm(state);
                    Ok(())
                }
                Err(gemini::GeminiError::Unauthorized) => Err("Google rejected this key".into()),
                Err(gemini::GeminiError::Network(_)) => Err("Couldn't reach Google".into()),
                Err(gemini::GeminiError::Server(_)) => Err("Google error, try again".into()),
            }
        }
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

/// The key was accepted: close the welcome card and hand the user to the
/// settings window. Without this the window simply disappears and a
/// first-time user cannot tell the app is still running.
#[tauri::command]
pub fn finish_first_run(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("firstrun") {
        let _ = w.hide();
    }
    crate::show_settings(&app);
    applog::log("first-run-complete");
}

/// What the microphone is really doing, for the settings window. `active` is
/// the device the stream actually opened, which differs from the user's
/// choice when Windows refused that device.
#[derive(serde::Serialize)]
pub struct MicStatus {
    pub healthy: bool,
    pub active: Option<String>,
}

#[tauri::command]
pub fn microphone_status(state: State<AppState>) -> MicStatus {
    MicStatus {
        healthy: state.audio.is_healthy(),
        active: state.audio.active_device(),
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

    #[test]
    fn vocabulary_is_trimmed_and_deduplicated_case_insensitively() {
        let value = vec![
            "Claude".to_string(),
            "claude ".to_string(),
            "".to_string(),
            "OpenAI".to_string(),
        ];

        assert_eq!(sanitize_vocabulary(value), vec!["Claude", "OpenAI"]);
    }

    #[test]
    fn model_names_reject_values_that_could_change_the_request_path() {
        assert_eq!(
            sanitized_model("gemini-3.5-transcribe", "fallback"),
            "gemini-3.5-transcribe"
        );
        assert_eq!(sanitized_model(" ../bad?key=x ", "fallback"), "fallback");
        assert_eq!(sanitized_model("", "fallback"), "fallback");
    }
}

/// The overlay reports the moment the listening bars are actually painted,
/// so hold-to-visible latency can be read straight from the log.
#[tauri::command]
pub fn overlay_visible() {
    applog::log("overlay-listening-visible");
}
