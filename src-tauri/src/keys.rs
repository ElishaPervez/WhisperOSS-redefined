//! Groq API key lookup. The key lives in Windows Credential Manager
//! (service "WhisperOSS", account "groq_api_key") and NEVER in a file.
//! Until the settings UI exists (M3), the WHISPEROSS_GROQ_KEY environment
//! variable bootstraps it: found once, it is saved into the vault.

use crate::applog;

const SERVICE: &str = "WhisperOSS";
const ACCOUNT: &str = "groq_api_key";
pub const ENV_VAR: &str = "WHISPEROSS_GROQ_KEY";

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

pub fn load() -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).ok();
    let store_val = entry.as_ref().and_then(|e| e.get_password().ok());
    let env_val = std::env::var(ENV_VAR).ok();
    let (key, save) = resolve(store_val, env_val);
    match (&key, save, &entry) {
        (Some(k), true, Some(e)) => {
            let _ = e.set_password(k);
            applog::log("api-key-bootstrapped-from-env");
        }
        (Some(_), false, _) => applog::log("api-key-from-credential-manager"),
        (None, _, _) => applog::log("api-key-missing"),
        _ => {}
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_wins_over_env() {
        assert_eq!(resolve(Some("sk_store".into()), Some("sk_env".into())),
                   (Some("sk_store".into()), false));
    }

    #[test]
    fn env_used_and_flagged_for_saving_when_store_empty() {
        assert_eq!(resolve(None, Some("  sk_env  ".into())),
                   (Some("sk_env".into()), true));
    }

    #[test]
    fn nothing_available() {
        assert_eq!(resolve(None, None), (None, false));
        assert_eq!(resolve(None, Some("   ".into())), (None, false));
    }
}
