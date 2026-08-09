//! Shared, live application state. Both the running dictation pipeline and
//! the settings-window commands read/write these behind mutexes, so a
//! setting changed in the window takes effect on the next dictation with no
//! restart. The key is kept separate from Config because it lives in
//! Credential Manager, never in config.json.

use std::sync::{Arc, Mutex};

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub key: Arc<Mutex<String>>,
}

impl AppState {
    pub fn new(config: Config, key: String) -> Self {
        AppState {
            config: Arc::new(Mutex::new(config)),
            key: Arc::new(Mutex::new(key)),
        }
    }
}
