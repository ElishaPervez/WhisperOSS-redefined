//! Gemini Live transcription. Microphone PCM is streamed while the hotkey is
//! held and the finalized transcript is returned after the stream-end signal.

use std::collections::VecDeque;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub const PROD_BASE_URL: &str = "https://generativelanguage.googleapis.com";
pub const PROD_WS_URL: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
const LIVE_FRAME_SAMPLES: usize = 1_600;
const LIVE_CHANNEL_CAPACITY: usize = 1_024;
const SESSION_REFRESH_AFTER: Duration = Duration::from_secs(9 * 60);
const FINAL_TRANSCRIPT_GRACE: Duration = Duration::from_millis(250);

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

enum LiveCommand {
    Start(LiveConfig, Option<std_mpsc::Sender<String>>),
    Audio(Vec<i16>, u32),
    Finish(std_mpsc::Sender<Result<String, GeminiError>>),
    Cancel,
}

#[derive(Clone)]
pub struct GeminiAudioSink {
    tx: mpsc::Sender<LiveCommand>,
}

impl GeminiAudioSink {
    pub fn push(&self, samples: Vec<i16>, source_rate: u32) {
        let _ = self.tx.try_send(LiveCommand::Audio(samples, source_rate));
    }
}

impl crate::audio::AudioStreamSink for GeminiAudioSink {
    fn push(&self, samples: Vec<i16>, source_rate: u32) {
        GeminiAudioSink::push(self, samples, source_rate);
    }
}

#[derive(Clone)]
pub struct GeminiLive {
    tx: mpsc::Sender<LiveCommand>,
}

impl GeminiLive {
    pub fn spawn(websocket_url: String, timeout: Duration) -> Self {
        let (tx, rx) = mpsc::channel(LIVE_CHANNEL_CAPACITY);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Gemini Live runtime");
            runtime.block_on(run_live_manager(rx, websocket_url, timeout));
        });
        Self { tx }
    }

    pub fn begin(
        &self,
        config: LiveConfig,
        stream_tx: Option<std_mpsc::Sender<String>>,
    ) -> Result<GeminiAudioSink, GeminiError> {
        self.tx
            .blocking_send(LiveCommand::Start(config, stream_tx))
            .map_err(|_| GeminiError::Network("live transcription worker stopped".into()))?;
        Ok(GeminiAudioSink {
            tx: self.tx.clone(),
        })
    }

    pub fn finish(&self) -> Result<std_mpsc::Receiver<Result<String, GeminiError>>, GeminiError> {
        let (result_tx, result_rx) = std_mpsc::channel();
        self.tx
            .blocking_send(LiveCommand::Finish(result_tx))
            .map_err(|_| GeminiError::Network("live transcription worker stopped".into()))?;
        Ok(result_rx)
    }

    pub fn cancel(&self) {
        let _ = self.tx.blocking_send(LiveCommand::Cancel);
    }
}

struct Utterance {
    config: LiveConfig,
    resampler: crate::dsp::StreamingResampler,
    pcm: Vec<i16>,
    sent_samples: usize,
    end_requested: bool,
    end_sent: bool,
    end_sent_at: Option<Instant>,
    completion_seen_at: Option<Instant>,
    server_turn_completed: bool,
    final_after_end: bool,
    cancelled: bool,
    retry_count: usize,
    transcript: TranscriptBuffer,
    stream_tx: Option<std_mpsc::Sender<String>>,
    result: Option<std_mpsc::Sender<Result<String, GeminiError>>>,
    terminal_result: Option<Result<String, GeminiError>>,
}

impl Utterance {
    fn new(config: LiveConfig, stream_tx: Option<std_mpsc::Sender<String>>) -> Self {
        Self {
            config,
            resampler: crate::dsp::StreamingResampler::new(),
            pcm: Vec::new(),
            sent_samples: 0,
            end_requested: false,
            end_sent: false,
            end_sent_at: None,
            completion_seen_at: None,
            server_turn_completed: false,
            final_after_end: false,
            cancelled: false,
            retry_count: 0,
            transcript: TranscriptBuffer::default(),
            stream_tx,
            result: None,
            terminal_result: None,
        }
    }
}

type LiveSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct LiveSession {
    socket: LiveSocket,
    config: LiveConfig,
    opened_at: Instant,
}

async fn run_live_manager(
    mut commands: mpsc::Receiver<LiveCommand>,
    websocket_url: String,
    timeout: Duration,
) {
    let mut queue = VecDeque::<Utterance>::new();
    let mut session: Option<LiveSession> = None;
    let mut maintenance_tick = tokio::time::interval(Duration::from_millis(25));

    loop {
        pump_front(&mut session, &mut queue, &websocket_url, timeout).await;

        if let Some(open) = session.as_mut() {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    handle_command(command, &mut queue);
                }
                message = open.socket.next() => {
                    handle_server_message(message, &mut session, &mut queue);
                }
                _ = maintenance_tick.tick() => {
                    if queue.front().and_then(|front| front.completion_seen_at)
                        .is_some_and(|seen_at| seen_at.elapsed() >= FINAL_TRANSCRIPT_GRACE)
                    {
                        let text = queue.front().unwrap().transcript.text();
                        complete_front(&mut queue, Ok(text));
                    } else if queue.front().and_then(|front| front.end_sent_at)
                        .is_some_and(|sent_at| sent_at.elapsed() >= timeout)
                    {
                        recover_or_complete(
                            &mut session,
                            &mut queue,
                            GeminiError::Network("Live finalization timed out".into()),
                        );
                    } else if queue.is_empty() && open.opened_at.elapsed() >= SESSION_REFRESH_AFTER {
                        session = None;
                    }
                }
            }
        } else {
            let Some(command) = commands.recv().await else {
                break;
            };
            handle_command(command, &mut queue);
        }
    }
}

fn handle_command(command: LiveCommand, queue: &mut VecDeque<Utterance>) {
    match command {
        LiveCommand::Start(config, stream_tx) => queue.push_back(Utterance::new(config, stream_tx)),
        LiveCommand::Audio(samples, rate) => {
            if let Some(utterance) = queue.back_mut().filter(|item| !item.end_requested) {
                utterance
                    .pcm
                    .extend(utterance.resampler.push(&samples, rate));
            }
        }
        LiveCommand::Finish(result) => match queue.back_mut() {
            Some(utterance) if utterance.terminal_result.is_some() => {
                let terminal = utterance.terminal_result.take().unwrap();
                let _ = result.send(terminal);
                queue.pop_back();
            }
            Some(utterance) if !utterance.end_requested => {
                utterance.end_requested = true;
                utterance.result = Some(result);
            }
            _ => {
                let _ = result.send(Err(GeminiError::Network(
                    "live transcription was not recording".into(),
                )));
            }
        },
        LiveCommand::Cancel => {
            if queue.len() > 1
                || queue
                    .back()
                    .is_some_and(|utterance| utterance.terminal_result.is_some())
            {
                queue.pop_back();
            } else if let Some(utterance) = queue.back_mut() {
                utterance.cancelled = true;
                utterance.end_requested = true;
            }
        }
    }
}

async fn pump_front(
    session: &mut Option<LiveSession>,
    queue: &mut VecDeque<Utterance>,
    websocket_url: &str,
    timeout: Duration,
) {
    let Some(front) = queue.front() else { return };
    if front.terminal_result.is_some() {
        return;
    }
    let needs_connection = session
        .as_ref()
        .map(|open| {
            open.config != front.config || open.opened_at.elapsed() >= SESSION_REFRESH_AFTER
        })
        .unwrap_or(true);
    if needs_connection {
        *session = None;
        match connect_live(websocket_url, &front.config, timeout).await {
            Ok(open) => *session = Some(open),
            Err(error) => {
                complete_front(queue, Err(error));
                return;
            }
        }
    }

    loop {
        let message = {
            let front = queue.front_mut().expect("front exists");
            if front.sent_samples + LIVE_FRAME_SAMPLES <= front.pcm.len() {
                let end = front.sent_samples + LIVE_FRAME_SAMPLES;
                let message = audio_message(&front.pcm[front.sent_samples..end]);
                front.sent_samples = end;
                Some(message)
            } else if front.end_requested && !front.end_sent {
                if front.sent_samples < front.pcm.len() {
                    let message = audio_message(&front.pcm[front.sent_samples..]);
                    front.sent_samples = front.pcm.len();
                    Some(message)
                } else {
                    front.end_sent = true;
                    front.end_sent_at = Some(Instant::now());
                    front.completion_seen_at = if front.server_turn_completed {
                        Some(Instant::now())
                    } else {
                        None
                    };
                    front.final_after_end = false;
                    Some(audio_stream_end_message())
                }
            } else {
                None
            }
        };
        let Some(message) = message else { break };
        let send_result = session
            .as_mut()
            .expect("session was connected")
            .socket
            .send(tungstenite::Message::Text(message.to_string().into()))
            .await;
        if let Err(error) = send_result {
            recover_or_complete(session, queue, websocket_error(error));
            break;
        }
    }
}

async fn connect_live(
    websocket_url: &str,
    config: &LiveConfig,
    timeout: Duration,
) -> Result<LiveSession, GeminiError> {
    let mut request = websocket_url
        .into_client_request()
        .map_err(|_| GeminiError::Network("invalid Live endpoint".into()))?;
    let key = tungstenite::http::HeaderValue::from_str(&config.key)
        .map_err(|_| GeminiError::Unauthorized)?;
    request.headers_mut().insert("x-goog-api-key", key);
    let (mut socket, _) = tokio::time::timeout(timeout, tokio_tungstenite::connect_async(request))
        .await
        .map_err(|_| GeminiError::Network("live connection timed out".into()))?
        .map_err(websocket_error)?;
    let setup = setup_message(&config.model, &config.vocabulary, config.smart);
    socket
        .send(tungstenite::Message::Text(setup.to_string().into()))
        .await
        .map_err(websocket_error)?;

    let setup_result = tokio::time::timeout(timeout, async {
        loop {
            match socket.next().await {
                Some(Ok(tungstenite::Message::Close(_))) | None => {
                    return Err(GeminiError::Network(
                        "Live connection closed during setup".into(),
                    ))
                }
                Some(Ok(message)) => {
                    if let Some(value) = decode_server_message(message)? {
                        match TranscriptBuffer::default().ingest(&value) {
                            ServerSignal::SetupComplete => return Ok(()),
                            ServerSignal::GoAway => {
                                return Err(GeminiError::Server("Live session unavailable".into()))
                            }
                            _ => {}
                        }
                    }
                }
                Some(Err(error)) => return Err(websocket_error(error)),
            }
        }
    })
    .await
    .map_err(|_| GeminiError::Network("Live setup timed out".into()))?;
    setup_result?;

    Ok(LiveSession {
        socket,
        config: config.clone(),
        opened_at: Instant::now(),
    })
}

fn handle_server_message(
    message: Option<Result<tungstenite::Message, tungstenite::Error>>,
    session: &mut Option<LiveSession>,
    queue: &mut VecDeque<Utterance>,
) {
    match message {
        Some(Ok(tungstenite::Message::Close(_))) | None => {
            recover_or_complete(
                session,
                queue,
                GeminiError::Network("Live connection closed".into()),
            );
        }
        Some(Ok(message)) => {
            let value = match decode_server_message(message) {
                Ok(Some(value)) => value,
                Ok(None) => return,
                Err(error) => {
                    recover_or_complete(session, queue, error);
                    return;
                }
            };
            let final_received = value["serverContent"]["inputTranscription"]["text"]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty());
            let mut completed_text = None;
            let signal = if let Some(front) = queue.front_mut() {
                let signal = front.transcript.ingest(&value);
                let current_text = front.transcript.text();
                if !current_text.is_empty() {
                    if let Some(ref tx) = front.stream_tx {
                        let _ = tx.send(current_text);
                    }
                }
                if front.end_sent && final_received {
                    front.final_after_end = true;
                }
                match signal {
                    ServerSignal::TurnComplete => {
                        front.server_turn_completed = true;
                        if front.end_sent {
                            completed_text = Some(front.transcript.text());
                        }
                    }
                    ServerSignal::GenerationComplete => {
                        front.server_turn_completed = true;
                        if front.end_sent {
                            if front.final_after_end {
                                completed_text = Some(front.transcript.text());
                            } else {
                                front.completion_seen_at = Some(Instant::now());
                            }
                        }
                    }
                    ServerSignal::Continue => {
                        if value.get("serverContent")
                            .and_then(|c| c.get("interimInputTranscription"))
                            .is_some()
                        {
                            front.server_turn_completed = false;
                        }
                        if front.end_sent
                            && final_received
                            && front.completion_seen_at.is_some()
                        {
                            completed_text = Some(front.transcript.text());
                        }
                    }
                    _ => {}
                }
                signal
            } else {
                TranscriptBuffer::default().ingest(&value)
            };
            if let Some(text) = completed_text {
                complete_front(queue, Ok(text));
            } else if signal == ServerSignal::GoAway {
                recover_or_complete(
                    session,
                    queue,
                    GeminiError::Network("Live session expired".into()),
                );
            }
        }
        Some(Err(error)) => recover_or_complete(session, queue, websocket_error(error)),
    }
}

fn recover_or_complete(
    session: &mut Option<LiveSession>,
    queue: &mut VecDeque<Utterance>,
    error: GeminiError,
) {
    *session = None;
    let Some(front) = queue.front_mut() else {
        return;
    };
    if front.retry_count == 0 && !matches!(error, GeminiError::Unauthorized) {
        front.retry_count += 1;
        front.sent_samples = 0;
        front.end_sent = false;
        front.end_sent_at = None;
        front.completion_seen_at = None;
        front.server_turn_completed = false;
        front.final_after_end = false;
        front.transcript = TranscriptBuffer::default();
    } else {
        complete_front(queue, Err(error));
    }
}

fn complete_front(queue: &mut VecDeque<Utterance>, result: Result<String, GeminiError>) {
    let Some(front) = queue.front_mut() else {
        return;
    };
    front.stream_tx = None;
    if front.cancelled {
        queue.pop_front();
    } else if let Some(sender) = front.result.take() {
        let _ = sender.send(result);
        queue.pop_front();
    } else {
        front.end_requested = true;
        front.terminal_result = Some(result);
    }
}

fn websocket_error(error: tungstenite::Error) -> GeminiError {
    match error {
        tungstenite::Error::Http(response)
            if matches!(response.status().as_u16(), 400 | 401 | 403) =>
        {
            GeminiError::Unauthorized
        }
        tungstenite::Error::Http(response) => GeminiError::Server(format!(
            "Live handshake HTTP {}",
            response.status().as_u16()
        )),
        _ => GeminiError::Network("Live WebSocket failed".into()),
    }
}

fn decode_server_message(
    message: tungstenite::Message,
) -> Result<Option<serde_json::Value>, GeminiError> {
    let value = match message {
        tungstenite::Message::Text(text) => serde_json::from_str(text.as_str()),
        tungstenite::Message::Binary(bytes) => serde_json::from_slice(bytes.as_ref()),
        _ => return Ok(None),
    }
    .map_err(|_| GeminiError::Server("invalid Live response".into()))?;
    Ok(Some(value))
}

fn setup_message(model: &str, vocabulary: &[String], smart: bool) -> serde_json::Value {
    let mut transcription = serde_json::json!({});
    if smart {
        transcription["mode"] = serde_json::json!("SMART");
    }
    if !vocabulary.is_empty() {
        transcription["customVocabulary"] = serde_json::json!(vocabulary);
    }
    serde_json::json!({
        "setup": {
            "model": format!("models/{model}"),
            "generationConfig": {
                "responseModalities": ["TEXT"],
            },
            "inputAudioTranscription": transcription,
        }
    })
}

fn audio_message(samples: &[i16]) -> serde_json::Value {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    serde_json::json!({
        "realtime_input": {
            "audio": {
                "data": base64::engine::general_purpose::URL_SAFE.encode(pcm),
                "mimeType": "audio/pcm;rate=16000",
            }
        }
    })
}

fn audio_stream_end_message() -> serde_json::Value {
    serde_json::json!({ "realtime_input": { "audioStreamEnd": true } })
}

#[derive(Debug, PartialEq, Eq)]
enum ServerSignal {
    Continue,
    SetupComplete,
    GenerationComplete,
    TurnComplete,
    GoAway,
}

#[derive(Default)]
struct TranscriptBuffer {
    finalized: Vec<String>,
    interim: String,
}

impl TranscriptBuffer {
    fn ingest(&mut self, message: &serde_json::Value) -> ServerSignal {
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
            if !text.is_empty() && !self.finalized.iter().any(|part| part == text) {
                self.finalized.push(text.to_string());
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

    fn text(&self) -> String {
        let mut parts = self.finalized.clone();
        let interim = self.interim.trim();
        if !interim.is_empty() && !parts.iter().any(|part| part == interim) {
            parts.push(interim.to_string());
        }
        parts.join(" ").trim().to_string()
    }
}

pub struct GeminiClient {
    http: reqwest::blocking::Client,
    base: String,
    key: String,
    model: String,
}

impl GeminiClient {
    pub fn new(key: String, model: String, base_url: String, timeout: Duration) -> GeminiClient {
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .no_proxy()
            .build()
            .expect("http client");
        GeminiClient {
            http,
            base: base_url.trim_end_matches('/').to_string(),
            key,
            model,
        }
    }

    pub fn validate_key(&self) -> Result<(), GeminiError> {
        let response = self
            .http
            .get(format!("{}/v1beta/models/{}", self.base, self.model))
            .header("x-goog-api-key", &self.key)
            .send()
            .map_err(|err| GeminiError::Network(err.to_string()))?;
        match response.status().as_u16() {
            200 => Ok(()),
            401 | 403 => Err(GeminiError::Unauthorized),
            status => Err(response_error(status, response)),
        }
    }
}

fn response_error(status: u16, response: reqwest::blocking::Response) -> GeminiError {
    let detail = response_detail(status, response);
    let normalized = detail.to_ascii_lowercase();
    if status == 400
        && (normalized.contains("api_key_invalid")
            || normalized.contains("api key not valid")
            || normalized.contains("invalid api key"))
    {
        GeminiError::Unauthorized
    } else {
        GeminiError::Server(detail)
    }
}

fn response_detail(status: u16, response: reqwest::blocking::Response) -> String {
    let body = response.text().unwrap_or_default();
    if body.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn client(base: String) -> GeminiClient {
        GeminiClient::new(
            "test-key".into(),
            crate::config::DEFAULT_GEMINI_MODEL.into(),
            base,
            Duration::from_secs(2),
        )
    }

    #[test]
    fn setup_selects_live_text_transcription_with_user_preferences() {
        let message = setup_message(
            "gemini-3.5-transcribe-live",
            &["WhisperOSS".into(), "Tauri".into()],
            true,
        );

        assert_eq!(
            message["setup"]["model"],
            "models/gemini-3.5-transcribe-live"
        );
        assert_eq!(
            message["setup"]["generationConfig"]["responseModalities"],
            serde_json::json!(["TEXT"])
        );
        assert_eq!(message["setup"]["inputAudioTranscription"]["mode"], "SMART");
        assert_eq!(
            message["setup"]["inputAudioTranscription"]["customVocabulary"],
            serde_json::json!(["WhisperOSS", "Tauri"])
        );
    }

    #[test]
    fn default_verbatim_setup_matches_minimal_live_sdk_request() {
        let message = setup_message("gemini-3.5-transcribe-live", &[], false);

        assert_eq!(
            message["setup"]["inputAudioTranscription"],
            serde_json::json!({})
        );
    }

    #[test]
    fn audio_frame_is_little_endian_pcm_at_16khz() {
        let message = audio_message(&[0, 1, -1]);

        assert_eq!(
            message["realtime_input"]["audio"]["mimeType"],
            "audio/pcm;rate=16000"
        );
        assert_eq!(message["realtime_input"]["audio"]["data"], "AAABAP__");
    }

    #[test]
    fn finalized_phrases_accumulate_across_pauses() {
        let mut transcript = TranscriptBuffer::default();
        assert_eq!(
            transcript.ingest(&serde_json::json!({
                "serverContent": { "interimInputTranscription": { "text": "first dra" } }
            })),
            ServerSignal::Continue
        );
        transcript.ingest(&serde_json::json!({
            "serverContent": { "inputTranscription": { "text": " first phrase " } }
        }));
        transcript.ingest(&serde_json::json!({
            "serverContent": { "interimInputTranscription": { "text": "second phrase" } }
        }));
        transcript.ingest(&serde_json::json!({
            "serverContent": {
                "inputTranscription": { "text": "second phrase" },
                "turnComplete": true
            }
        }));

        assert_eq!(transcript.text(), "first phrase second phrase");
    }

    #[test]
    fn unfinished_interim_is_not_lost_at_stream_end() {
        let mut transcript = TranscriptBuffer::default();
        transcript.ingest(&serde_json::json!({
            "serverContent": { "interimInputTranscription": { "text": "trailing words" } }
        }));

        assert_eq!(transcript.text(), "trailing words");
    }

    #[test]
    fn generation_complete_does_not_beat_late_transcription() {
        let mut transcript = TranscriptBuffer::default();

        assert_eq!(
            transcript.ingest(&serde_json::json!({
                "serverContent": { "generationComplete": true }
            })),
            ServerSignal::GenerationComplete
        );
        transcript.ingest(&serde_json::json!({
            "serverContent": {
                "inputTranscription": { "text": "arrived after generation" },
                "turnComplete": true
            }
        }));
        assert_eq!(transcript.text(), "arrived after generation");
    }

    #[test]
    fn live_manager_streams_two_utterances_over_one_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut websocket = tokio_tungstenite::tungstenite::accept_hdr(
                tcp,
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                 response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                    assert_eq!(request.headers()["x-goog-api-key"], "test-key");
                    Ok(response)
                },
            )
            .unwrap();
            let setup: serde_json::Value =
                serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                    .unwrap();
            assert_eq!(setup["setup"]["model"], "models/gemini-3.5-transcribe-live");
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Binary(
                    br#"{"setupComplete":{}}"#.to_vec().into(),
                ))
                .unwrap();

            for (index, expected) in ["first result", "second result"].into_iter().enumerate() {
                let mut audio_frames = 0;
                loop {
                    let value: serde_json::Value = serde_json::from_str(
                        websocket.read().unwrap().into_text().unwrap().as_str(),
                    )
                    .unwrap();
                    if value["realtime_input"]["audio"].is_object() {
                        audio_frames += 1;
                    }
                    if value["realtime_input"]["audioStreamEnd"].as_bool() == Some(true) {
                        break;
                    }
                }
                assert_eq!(audio_frames, 1);
                if index == 0 {
                    websocket
                        .send(tokio_tungstenite::tungstenite::Message::Binary(
                            serde_json::json!({
                                "serverContent": { "generationComplete": true }
                            })
                            .to_string()
                            .into_bytes()
                            .into(),
                        ))
                        .unwrap();
                    std::thread::sleep(Duration::from_millis(20));
                    websocket
                        .send(tokio_tungstenite::tungstenite::Message::Binary(
                            serde_json::json!({
                                "serverContent": {
                                    "inputTranscription": { "text": expected }
                                }
                            })
                            .to_string()
                            .into_bytes()
                            .into(),
                        ))
                        .unwrap();
                } else {
                    websocket
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            serde_json::json!({
                                "serverContent": {
                                    "inputTranscription": { "text": expected },
                                    "turnComplete": true
                                }
                            })
                            .to_string()
                            .into(),
                        ))
                        .unwrap();
                }
            }
        });

        let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_secs(2));
        let config = LiveConfig {
            key: "test-key".into(),
            model: "gemini-3.5-transcribe-live".into(),
            vocabulary: vec!["WhisperOSS".into()],
            smart: false,
        };
        for expected in ["first result", "second result"] {
            let sink = live.begin(config.clone(), None).unwrap();
            sink.push(vec![700; 1_600], 16_000);
            let result = live
                .finish()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            assert_eq!(result, expected);
        }
        server.join().unwrap();
    }

    #[test]
    fn live_manager_streams_interim_chunks() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut websocket = tokio_tungstenite::tungstenite::accept(tcp).unwrap();
            let _ = websocket.read().unwrap();
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    r#"{"setupComplete":{}}"#.into(),
                ))
                .unwrap();
            let _ = websocket.read().unwrap();
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    r#"{"serverContent":{"interimInputTranscription":{"text":"hello"}}}"#.into(),
                ))
                .unwrap();
            let _ = websocket.read().unwrap();
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    r#"{"serverContent":{"interimInputTranscription":{"text":"hello world"}}}"#.into(),
                ))
                .unwrap();
            loop {
                let value: serde_json::Value =
                    serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                        .unwrap();
                if value["realtime_input"]["audioStreamEnd"].as_bool() == Some(true) {
                    break;
                }
            }
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::json!({
                        "serverContent": {
                            "inputTranscription": { "text": "hello world" },
                            "turnComplete": true
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .unwrap();
        });

        let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_secs(2));
        let (stream_tx, stream_rx) = std::sync::mpsc::channel();
        let sink = live
            .begin(
                LiveConfig {
                    key: "test-key".into(),
                    model: "gemini-3.5-transcribe-live".into(),
                    vocabulary: Vec::new(),
                    smart: false,
                },
                Some(stream_tx),
            )
            .unwrap();
        sink.push(vec![700; 1_600], 16_000);
        let first = stream_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(first, "hello");
        sink.push(vec![700; 1_600], 16_000);
        let second = stream_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(second, "hello world");
        let result = live
            .finish()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(result, "hello world");
        server.join().unwrap();
    }

    #[test]
    fn live_manager_reports_when_final_transcript_never_arrives() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut websocket = tokio_tungstenite::tungstenite::accept(tcp).unwrap();
            let _ = websocket.read().unwrap();
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    r#"{"setupComplete":{}}"#.into(),
                ))
                .unwrap();
            loop {
                let value: serde_json::Value =
                    serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                        .unwrap();
                if value["realtime_input"]["audioStreamEnd"].as_bool() == Some(true) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(1_500));
        });

        let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_millis(100));
        let sink = live
            .begin(
                LiveConfig {
                    key: "test-key".into(),
                    model: "gemini-3.5-transcribe-live".into(),
                    vocabulary: Vec::new(),
                    smart: false,
                },
                None,
            )
            .unwrap();
        sink.push(vec![700; 1_600], 16_000);
        let result = live
            .finish()
            .unwrap()
            .recv_timeout(Duration::from_millis(500))
            .unwrap();

        assert!(matches!(result, Err(GeminiError::Network(_))));
        server.join().unwrap();
    }

    #[test]
    fn setup_failure_is_preserved_until_recording_finishes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut websocket = tokio_tungstenite::tungstenite::accept(tcp).unwrap();
            let _ = websocket.read().unwrap();
            websocket.close(None).unwrap();
        });

        let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_millis(200));
        let sink = live
            .begin(
                LiveConfig {
                    key: "test-key".into(),
                    model: "gemini-3.5-transcribe-live".into(),
                    vocabulary: Vec::new(),
                    smart: false,
                },
                None,
            )
            .unwrap();
        sink.push(vec![700; 1_600], 16_000);
        std::thread::sleep(Duration::from_millis(250));
        let result = live
            .finish()
            .unwrap()
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        assert!(matches!(
            result,
            Err(GeminiError::Network(detail))
                if detail == "Live connection closed during setup"
        ));
    }

    #[test]
    fn rejected_key_is_reported_during_validation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf);
            socket
                .write_all(b"HTTP/1.1 403 Forbidden\r\nconnection: close\r\n\r\n{}")
                .unwrap();
        });
        assert!(matches!(
            client(format!("http://{addr}")).validate_key(),
            Err(GeminiError::Unauthorized)
        ));
    }

    #[test]
    fn live_manager_completes_when_turn_completed_during_speech_pause() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut websocket = tokio_tungstenite::tungstenite::accept(tcp).unwrap();
            let _ = websocket.read().unwrap(); // setup
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    r#"{"setupComplete":{}}"#.into(),
                ))
                .unwrap();
            let _ = websocket.read().unwrap(); // first audio chunk
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::json!({
                        "serverContent": {
                            "inputTranscription": { "text": "early finished text" },
                            "turnComplete": true
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .unwrap();
            // Server receives trailing silence audio and audioStreamEnd, but sends nothing more
            loop {
                let msg = websocket.read().unwrap();
                if let Ok(text) = msg.into_text() {
                    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                    if val["realtime_input"]["audioStreamEnd"].as_bool() == Some(true) {
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(800));
        });

        let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_secs(5));
        let sink = live
            .begin(
                LiveConfig {
                    key: "test-key".into(),
                    model: "gemini-3.5-transcribe-live".into(),
                    vocabulary: Vec::new(),
                    smart: false,
                },
                None,
            )
            .unwrap();
        sink.push(vec![700; 1_600], 16_000);
        // Wait long enough for the server turnComplete to be processed while still recording
        std::thread::sleep(Duration::from_millis(150));
        // Push 2 more seconds of silence
        sink.push(vec![0; 1_600], 16_000);
        // Release key (finish)
        let result = live
            .finish()
            .unwrap()
            .recv_timeout(Duration::from_millis(800))
            .unwrap()
            .unwrap();

        assert_eq!(result, "early finished text");
        server.join().unwrap();
    }

    #[test]
    fn google_style_400_invalid_key_is_unauthorized() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf);
            socket
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nconnection: close\r\n\r\n{\"error\":{\"status\":\"API_KEY_INVALID\"}}",
                )
                .unwrap();
        });
        assert!(matches!(
            client(format!("http://{addr}")).validate_key(),
            Err(GeminiError::Unauthorized)
        ));
    }

    /// Manual service-contract check. It is ignored during ordinary tests so
    /// the suite never requires a key or sends audio unless explicitly asked.
    #[test]
    #[ignore]
    fn live_service_transcribes_pcm_fixture() {
        let key = std::env::var("WHISPEROSS_TEST_GEMINI_KEY").expect("test key missing");
        let pcm_path = std::env::var("WHISPEROSS_TEST_PCM_PATH").expect("PCM fixture path missing");
        let pcm = std::fs::read(pcm_path).expect("PCM fixture unreadable");
        assert_eq!(pcm.len() % 2, 0, "PCM fixture must contain 16-bit samples");
        let samples = pcm
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        let live = GeminiLive::spawn(PROD_WS_URL.into(), Duration::from_secs(30));
        let sink = live
            .begin(
                LiveConfig {
                    key,
                    model: crate::config::DEFAULT_GEMINI_MODEL.into(),
                    vocabulary: Vec::new(),
                    smart: false,
                },
                None,
            )
            .unwrap();
        for chunk in samples.chunks(LIVE_FRAME_SAMPLES) {
            sink.push(chunk.to_vec(), 16_000);
            std::thread::sleep(Duration::from_millis(100));
        }
        let transcript = live
            .finish()
            .unwrap()
            .recv_timeout(Duration::from_secs(30))
            .unwrap()
            .unwrap();
        assert!(!transcript.is_empty(), "Google returned no transcript");
    }
}
