//! Provider API keys live in separate Windows Credential Manager entries and
//! NEVER in config.json. Environment variables can bootstrap an empty entry.

use crate::applog;

const SERVICE: &str = "WhisperOSS";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Groq,
    Gemini,
}

impl Provider {
    fn account(self) -> &'static str {
        match self {
            Self::Groq => "groq_api_key",
            Self::Gemini => "gemini_api_key",
        }
    }

    fn env_var(self) -> &'static str {
        match self {
            Self::Groq => "WHISPEROSS_GROQ_KEY",
            Self::Gemini => "GEMINI_API_KEY",
        }
    }

    fn log_name(self) -> &'static str {
        match self {
            Self::Groq => "groq",
            Self::Gemini => "gemini",
        }
    }
}

/// Pure decision: which key to use, and whether it should be persisted.
pub fn resolve(store_val: Option<String>, env_val: Option<String>) -> (Option<String>, bool) {
    if let Some(k) = store_val {
        if !k.trim().is_empty() {
            return (Some(k), false);
        }
    }
    match env_val {
        Some(k) if !k.trim().is_empty() => (Some(k.trim().to_string()), true),
        _ => (None, false),
    }
}

pub fn load(provider: Provider) -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, provider.account()).ok();
    let store_val = entry.as_ref().and_then(|e| e.get_password().ok());
    let env_val = std::env::var(provider.env_var()).ok();
    let (key, save) = resolve(store_val, env_val);
    match (&key, save, &entry) {
        (Some(k), true, Some(e)) => {
            let _ = e.set_password(k);
            applog::log(&format!(
                "{}-key-bootstrapped-from-env",
                provider.log_name()
            ));
        }
        (Some(_), false, _) => applog::log(&format!(
            "{}-key-from-credential-manager",
            provider.log_name()
        )),
        (None, _, _) => applog::log(&format!("{}-key-missing", provider.log_name())),
        _ => {}
    }
    key
}

/// Pure decision for dictation time: a non-empty vault value wins over the
/// in-memory snapshot; anything else keeps the snapshot. Returns the key to
/// use and whether it replaced the snapshot.
pub fn refreshed_key(current: &str, vault_val: Option<String>) -> (String, bool) {
    match vault_val {
        Some(v) if !v.trim().is_empty() => {
            let changed = v != current;
            (v, changed)
        }
        _ => (current.to_string(), false),
    }
}

/// Quiet vault read for the per-dictation refresh: no logging, no env
/// bootstrap (those are startup concerns handled by `load`).
pub fn read_vault(provider: Provider) -> Option<String> {
    keyring::Entry::new(SERVICE, provider.account())
        .ok()?
        .get_password()
        .ok()
}

/// Persist the key into Windows Credential Manager. Returns false on failure.
#[allow(dead_code)]
pub fn save(provider: Provider, key: &str) -> bool {
    match keyring::Entry::new(SERVICE, provider.account()) {
        Ok(entry) => {
            let ok = entry.set_password(key).is_ok();
            if ok {
                applog::log(&format!("{}-key-saved", provider.log_name()));
            } else {
                applog::log(&format!("{}-key-save-failed", provider.log_name()));
            }
            ok
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_wins_over_env() {
        assert_eq!(
            resolve(Some("sk_store".into()), Some("sk_env".into())),
            (Some("sk_store".into()), false)
        );
    }

    #[test]
    fn env_used_and_flagged_for_saving_when_store_empty() {
        assert_eq!(
            resolve(None, Some("  sk_env  ".into())),
            (Some("sk_env".into()), true)
        );
    }

    #[test]
    fn nothing_available() {
        assert_eq!(resolve(None, None), (None, false));
        assert_eq!(resolve(None, Some("   ".into())), (None, false));
    }

    #[test]
    fn refresh_prefers_new_vault_key_and_flags_the_change() {
        assert_eq!(
            refreshed_key("sk_old", Some("sk_new".into())),
            ("sk_new".into(), true)
        );
    }

    #[test]
    fn refresh_with_unchanged_vault_key_is_not_a_change() {
        assert_eq!(
            refreshed_key("sk_same", Some("sk_same".into())),
            ("sk_same".into(), false)
        );
    }

    #[test]
    fn refresh_keeps_memory_key_when_vault_is_unreadable_or_blank() {
        // A failed or cleared vault read must never wipe a working key
        // mid-session — dictation falls back to the launch-time snapshot.
        assert_eq!(refreshed_key("sk_mem", None), ("sk_mem".into(), false));
        assert_eq!(
            refreshed_key("sk_mem", Some("   ".into())),
            ("sk_mem".into(), false)
        );
    }
}
