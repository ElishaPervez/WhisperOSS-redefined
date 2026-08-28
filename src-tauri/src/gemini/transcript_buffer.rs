#[derive(Debug, PartialEq, Eq)]
pub enum ServerSignal {
    Continue,
    SetupComplete,
    GenerationComplete,
    TurnComplete,
    GoAway,
}

#[derive(Default)]
pub struct TranscriptBuffer {
    finalized: String,
    interim: String,
}

impl TranscriptBuffer {
    pub fn ingest(&mut self, message: &serde_json::Value) -> ServerSignal {
        if message.get("setupComplete").is_some() {
            return ServerSignal::SetupComplete;
        }
        if message.get("goAway").is_some() {
            return ServerSignal::GoAway;
        }
        let Some(content) = message.get("serverContent") else {
            return ServerSignal::Continue;
        };
        if let Some(text) = content["interimInputTranscription"]["text"].as_str() {
            self.interim = text.trim().to_string();
        }
        if let Some(text) = content["inputTranscription"]["text"].as_str() {
            let text = text.trim();
            if !text.is_empty() {
                self.finalized = text.to_string();
            }
            self.interim.clear();
        }
        if content["turnComplete"].as_bool() == Some(true) {
            ServerSignal::TurnComplete
        } else if content["generationComplete"].as_bool() == Some(true) {
            ServerSignal::GenerationComplete
        } else {
            ServerSignal::Continue
        }
    }

    pub fn text(&self) -> String {
        if !self.finalized.is_empty() {
            self.finalized.clone()
        } else {
            self.interim.clone()
        }
    }
}
