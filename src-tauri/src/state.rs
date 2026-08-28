//! Shared, live application state. Both the running dictation pipeline and
//! the settings-window commands read/write these behind mutexes, so a
//! setting changed in the window takes effect on the next dictation with no
//! restart. Provider keys are kept separate from Config because they live in
//! Credential Manager, never in config.json.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use crate::audio::AudioEngine;
use crate::config::Config;
use crate::gemini::GeminiLive;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub groq_key: Arc<Mutex<String>>,
    pub gemini_key: Arc<Mutex<String>>,
    /// The running mic stream. Changing the device swaps it in place.
    pub audio: Arc<AudioEngine>,
    /// Bumped whenever a dictation is superseded; a stale worker checks this
    /// before it touches the pill.
    pub generation: Arc<AtomicU64>,
    pub gemini_live: GeminiLive,
}

impl AppState {
    pub fn new(
        config: Config,
        groq_key: String,
        gemini_key: String,
        audio: Arc<AudioEngine>,
        gemini_live: GeminiLive,
    ) -> Self {
        AppState {
            config: Arc::new(Mutex::new(config)),
            groq_key: Arc::new(Mutex::new(groq_key)),
            gemini_key: Arc::new(Mutex::new(gemini_key)),
            audio,
            generation: Arc::new(AtomicU64::new(0)),
            gemini_live,
        }
    }
}
