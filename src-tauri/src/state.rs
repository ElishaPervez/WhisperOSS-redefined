//! Shared, live application state. Both the running dictation pipeline and
//! the settings-window commands read/write these behind mutexes, so a
//! setting changed in the window takes effect on the next dictation with no
//! restart. The key is kept separate from Config because it lives in
//! Credential Manager, never in config.json.

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use crate::audio::AudioEngine;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub key: Arc<Mutex<String>>,
    /// The running mic stream. Changing the device swaps it in place.
    pub audio: Arc<AudioEngine>,
    /// Bumped whenever a dictation is superseded; a stale worker checks this
    /// before it touches the pill.
    pub generation: Arc<AtomicU64>,
    /// True from "Change hotkey" until the combo lands, is refused, or times
    /// out. While true the machine's keyboard is being swallowed.
    pub capturing: Arc<AtomicBool>,
    /// Stops a watchdog from cancelling a capture session it did not start.
    pub capture_gen: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(config: Config, key: String, audio: Arc<AudioEngine>) -> Self {
        AppState {
            config: Arc::new(Mutex::new(config)),
            key: Arc::new(Mutex::new(key)),
            audio,
            generation: Arc::new(AtomicU64::new(0)),
            capturing: Arc::new(AtomicBool::new(false)),
            capture_gen: Arc::new(AtomicU64::new(0)),
        }
    }
}
