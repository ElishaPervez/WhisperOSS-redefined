//! Gemini Live transcription & REST client modules.
//!
//! Submodules:
//! - `client`: REST client for key validation and model inspection.
//! - `protocol`: Wire formats, message serialization, and WebSocket decoding.
//! - `transcript_buffer`: State machine tracking interim vs finalized text tokens.
//! - `live`: Streaming WebSocket actor, connection lifecycle, pre-warming, and queueing.

mod client;
mod live;
mod protocol;
mod transcript_buffer;

#[cfg(test)]
mod tests;

pub use client::GeminiClient;
pub use live::GeminiLive;
#[allow(unused_imports)]
pub use live::GeminiAudioSink;
pub use protocol::{PROD_BASE_URL, PROD_WS_URL};
#[cfg(test)]
#[allow(unused_imports)]
pub use transcript_buffer::{ServerSignal, TranscriptBuffer};

#[derive(Debug)]
pub enum GeminiError {
    Unauthorized,
    Network(String),
    Server(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveConfig {
    pub key: String,
    pub model: String,
    pub vocabulary: Vec<String>,
    pub smart: bool,
}
