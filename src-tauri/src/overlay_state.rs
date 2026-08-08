//! The overlay's state contract. Rust owns ALL timing and wording; the
//! webview only renders. States: listening | processing | success | error
//! | hidden. Durations come from the approved design (art2-pill.png).

use crate::groq::GroqError;

pub const FADE_MS: u64 = 240;
pub const SUCCESS_HOLD_MS: u64 = 400;
pub const ERROR_HOLD_MS: u64 = 2000;

pub fn ui_payload(state: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "state": state, "message": message })
}

/// Spec §6 error table: (short pill message, detail for the log).
/// Reading the detail here is also what finally consumes the error
/// payloads M1 left unread (the old dead-code warnings).
pub fn describe_error(err: &GroqError) -> (&'static str, String) {
    match err {
        GroqError::Unauthorized => ("Invalid API key", String::new()),
        GroqError::Network(detail) => ("Couldn't reach Groq", detail.clone()),
        GroqError::Server(detail) => ("Groq error", detail.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groq::GroqError;

    #[test]
    fn payload_shape() {
        let p = ui_payload("error", "Couldn't reach Groq");
        assert_eq!(p["state"], "error");
        assert_eq!(p["message"], "Couldn't reach Groq");
        let p = ui_payload("listening", "");
        assert_eq!(p["message"], "");
    }

    #[test]
    fn error_descriptions_match_spec() {
        let (msg, detail) = describe_error(&GroqError::Unauthorized);
        assert_eq!(msg, "Invalid API key");
        assert_eq!(detail, "");

        let (msg, detail) = describe_error(&GroqError::Network("dns fail".into()));
        assert_eq!(msg, "Couldn't reach Groq");
        assert_eq!(detail, "dns fail");

        let (msg, detail) = describe_error(&GroqError::Server("HTTP 500".into()));
        assert_eq!(msg, "Groq error");
        assert_eq!(detail, "HTTP 500");
    }

    #[test]
    fn durations_match_design() {
        assert_eq!(FADE_MS, 240);
        assert_eq!(SUCCESS_HOLD_MS, 400);
        assert_eq!(ERROR_HOLD_MS, 2000);
    }
}
