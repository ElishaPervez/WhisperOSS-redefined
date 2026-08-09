//! Persisted settings (spec §4). Every field has a default and the file is
//! corrupt-tolerant: a bad or missing config.json can never brick the app.
//! The API key is NOT here — it lives in Windows Credential Manager (M1).

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Hold-to-dictate combo: lowercase key names, e.g. ["ctrl","win"] or
    /// ["ctrl","space"]. Validated by hotkey_logic::parse_combo.
    pub hotkey: Vec<String>,
    pub use_formatter: bool,
    pub casual_mode: bool,
    /// Input device by NAME (indexes shift between sessions). None = default.
    pub input_device: Option<String>,
    /// "auto" | "light" | "dark" (consumed by the settings window in M3b).
    pub theme: String,
    pub run_on_startup: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: vec!["ctrl".into(), "win".into()],
            use_formatter: false,
            casual_mode: false,
            input_device: None,
            theme: "auto".into(),
            run_on_startup: true,
        }
    }
}

pub fn from_json(text: &str) -> Config {
    serde_json::from_str(text).unwrap_or_default()
}

pub fn to_json(cfg: &Config) -> String {
    serde_json::to_string_pretty(cfg).expect("config serializes")
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(base).join("WhisperOSS");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("config.json"))
}

pub fn load() -> Config {
    config_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|t| from_json(&t))
        .unwrap_or_default()
}

pub fn save(cfg: &Config) {
    if let Some(p) = config_path() {
        let _ = fs::write(p, to_json(cfg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(c.hotkey, vec!["ctrl".to_string(), "win".to_string()]);
        assert!(!c.use_formatter);
        assert!(!c.casual_mode);
        assert_eq!(c.input_device, None);
        assert_eq!(c.theme, "auto");
        assert!(c.run_on_startup);
    }

    #[test]
    fn partial_json_fills_missing_fields_with_defaults() {
        let c = from_json(r#"{ "use_formatter": true }"#);
        assert!(c.use_formatter);
        assert_eq!(c.hotkey, vec!["ctrl".to_string(), "win".to_string()]);
        assert_eq!(c.theme, "auto");
    }

    #[test]
    fn corrupt_json_falls_back_to_defaults() {
        let c = from_json("{ not valid json !!");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn roundtrip() {
        let mut c = Config::default();
        c.casual_mode = true;
        c.input_device = Some("Yeti Nano (WASAPI)".into());
        assert_eq!(from_json(&to_json(&c)), c);
    }
}
