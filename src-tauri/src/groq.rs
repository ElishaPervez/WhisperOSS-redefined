//! Groq speech-to-text over plain REST (no SDK — spec §4). Hard rules:
//! 15 s timeout, exactly one retry on network/server failures, no retry on
//! a rejected key, cancellation handled by the caller via generation check.

use std::time::Duration;

pub const PROD_BASE_URL: &str = "https://api.groq.com";
const MODEL: &str = "whisper-large-v3-turbo";
const FORMAT_MODEL: &str = "openai/gpt-oss-120b";

#[derive(Debug)]
pub enum GroqError {
    Unauthorized,
    Network(String),
    Server(String),
}

pub struct GroqClient {
    http: reqwest::blocking::Client,
    base: String,
    key: String,
}

impl GroqClient {
    pub fn new(key: String, base_url: String, timeout: Duration) -> GroqClient {
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .expect("http client");
        GroqClient { http, base: base_url, key }
    }

    pub fn transcribe(&self, wav: Vec<u8>, vocab_prompt: &str) -> Result<String, GroqError> {
        let mut last = None;
        for _ in 0..2 {
            match self.attempt(wav.clone(), vocab_prompt) {
                Ok(text) => return Ok(text),
                Err(GroqError::Unauthorized) => return Err(GroqError::Unauthorized),
                Err(e) => last = Some(e),
            }
        }
        Err(last.expect("at least one attempt ran"))
    }

    fn attempt(&self, wav: Vec<u8>, vocab_prompt: &str) -> Result<String, GroqError> {
        let part = reqwest::blocking::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| GroqError::Network(e.to_string()))?;
        let form = reqwest::blocking::multipart::Form::new()
            .part("file", part)
            .text("model", MODEL)
            .text("language", "en")
            .text("temperature", "0")
            .text("response_format", "json");
        let form = if vocab_prompt.is_empty() {
            form
        } else {
            form.text("prompt", vocab_prompt.to_string())
        };

        let resp = self
            .http
            .post(format!("{}/openai/v1/audio/transcriptions", self.base))
            .bearer_auth(&self.key)
            .multipart(form)
            .send()
            .map_err(|e| GroqError::Network(e.to_string()))?;

        match resp.status().as_u16() {
            200 => {
                let v: serde_json::Value =
                    resp.json().map_err(|e| GroqError::Network(e.to_string()))?;
                Ok(v["text"].as_str().unwrap_or_default().trim().to_string())
            }
            401 | 403 => Err(GroqError::Unauthorized),
            s => Err(GroqError::Server(format!("HTTP {s}"))),
        }
    }

    /// Optional formal cleanup pass (spec §2). Same retry discipline as
    /// transcribe. Casual mode never comes here — it is a local rewrite
    /// (see casualize.rs).
    pub fn format_text(&self, text: &str) -> Result<String, GroqError> {
        let mut last = None;
        for _ in 0..2 {
            match self.format_attempt(text) {
                Ok(t) => return Ok(t),
                Err(GroqError::Unauthorized) => return Err(GroqError::Unauthorized),
                Err(e) => last = Some(e),
            }
        }
        Err(last.expect("at least one attempt ran"))
    }

    fn format_attempt(&self, text: &str) -> Result<String, GroqError> {
        let prompt = crate::prompts::FORMAT_PROMPT;
        let body = serde_json::json!({
            "model": FORMAT_MODEL,
            "temperature": 0.3,
            "messages": [
                { "role": "system", "content": prompt },
                { "role": "user", "content": text },
            ],
        });
        let resp = self
            .http
            .post(format!("{}/openai/v1/chat/completions", self.base))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .map_err(|e| GroqError::Network(e.to_string()))?;
        match resp.status().as_u16() {
            200 => {
                let v: serde_json::Value =
                    resp.json().map_err(|e| GroqError::Network(e.to_string()))?;
                Ok(v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string())
            }
            401 | 403 => Err(GroqError::Unauthorized),
            s => Err(GroqError::Server(format!("HTTP {s}"))),
        }
    }

    /// Cheap key check for the settings Save button (M3b).
    #[allow(dead_code)]
    pub fn validate_key(&self) -> Result<(), GroqError> {
        let resp = self
            .http
            .get(format!("{}/openai/v1/models", self.base))
            .bearer_auth(&self.key)
            .send()
            .map_err(|e| GroqError::Network(e.to_string()))?;
        match resp.status().as_u16() {
            200 => Ok(()),
            401 | 403 => Err(GroqError::Unauthorized),
            s => Err(GroqError::Server(format!("HTTP {s}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::Duration;

    /// One-shot HTTP server: accepts a single connection, reads the request,
    /// writes `response` verbatim. Close-delimited bodies (no content-length).
    fn serve_once(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 65536];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(response.as_bytes());
        });
        format!("http://{addr}")
    }

    fn serve_once_capturing(response: &'static str) -> (String, Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 65536];
            let bytes_read = sock.read(&mut buf).unwrap();
            sender.send(buf[..bytes_read].to_vec()).unwrap();
            let _ = sock.write_all(response.as_bytes());
        });
        (format!("http://{addr}"), receiver)
    }

    fn client(base: String) -> GroqClient {
        GroqClient::new("test-key".into(), base, Duration::from_secs(2))
    }

    #[test]
    fn parses_transcription_text() {
        let base = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"text\": \" hello world \"}",
        );
        assert_eq!(client(base).transcribe(vec![0u8; 16], "").unwrap(), "hello world");
    }

    #[test]
    fn transcription_includes_non_empty_vocabulary_prompt() {
        let (base, request) = serve_once_capturing(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"text\": \"ok\"}",
        );
        let prompt = "Codex, Claude, OpenAI, Anthropic, Fable";

        assert_eq!(client(base).transcribe(vec![0u8; 16], prompt).unwrap(), "ok");
        let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
        let request = String::from_utf8_lossy(&request);
        assert!(request.contains(&format!("name=\"prompt\"\r\n\r\n{prompt}\r\n")));
    }

    #[test]
    fn transcription_omits_empty_vocabulary_prompt() {
        let (base, request) = serve_once_capturing(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"text\": \"ok\"}",
        );

        assert_eq!(client(base).transcribe(vec![0u8; 16], "").unwrap(), "ok");
        let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
        let request = String::from_utf8_lossy(&request);
        assert!(!request.contains("name=\"prompt\""));
    }

    #[test]
    fn unauthorized_maps_and_does_not_retry() {
        // serve_once accepts exactly one connection; a retry would fail with
        // a network error instead of Unauthorized — so this also proves no retry.
        let base = serve_once("HTTP/1.1 401 Unauthorized\r\nconnection: close\r\n\r\n{}");
        assert!(matches!(client(base).transcribe(vec![0u8; 16], ""),
                         Err(GroqError::Unauthorized)));
    }

    #[test]
    fn retries_once_after_dropped_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            drop(sock); // first attempt: connection dies
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 65536];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"text\": \"second try\"}",
            );
        });
        let c = client(format!("http://{addr}"));
        assert_eq!(c.transcribe(vec![0u8; 16], "").unwrap(), "second try");
    }

    #[test]
    fn server_error_after_retries_maps_to_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for _ in 0..2 {
                let (mut sock, _) = listener.accept().unwrap();
                let mut buf = [0u8; 65536];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(b"HTTP/1.1 500 Oops\r\nconnection: close\r\n\r\n");
            }
        });
        let c = client(format!("http://{addr}"));
        assert!(matches!(c.transcribe(vec![0u8; 16], ""), Err(GroqError::Server(_))));
    }

    #[test]
    fn format_text_sends_chat_and_returns_content() {
        let base = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"choices\":[{\"message\":{\"content\":\" Hello, world. \"}}]}",
        );
        let c = client(base);
        assert_eq!(c.format_text("hello world").unwrap(), "Hello, world.");
    }

    #[test]
    fn format_text_unauthorized_maps() {
        let base = serve_once("HTTP/1.1 401 Unauthorized\r\nconnection: close\r\n\r\n{}");
        assert!(matches!(client(base).format_text("x"),
                         Err(GroqError::Unauthorized)));
    }

    #[test]
    fn validate_key_ok_and_unauthorized() {
        let base = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"data\":[]}",
        );
        assert!(client(base).validate_key().is_ok());
        let base = serve_once("HTTP/1.1 401 Unauthorized\r\nconnection: close\r\n\r\n{}");
        assert!(matches!(client(base).validate_key(),
                         Err(GroqError::Unauthorized)));
    }
}
