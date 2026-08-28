use std::collections::VecDeque;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::protocol::{
    activity_end_message, activity_start_message, audio_message, decode_server_message,
    setup_message, websocket_error,
};
use super::transcript_buffer::{ServerSignal, TranscriptBuffer};
use super::{GeminiError, LiveConfig};

const LIVE_FRAME_SAMPLES: usize = 640; // 40 ms at 16 kHz
const LIVE_CHANNEL_CAPACITY: usize = 1_024;
const SESSION_REFRESH_AFTER: Duration = Duration::from_secs(9 * 60);
const FINAL_TRANSCRIPT_GRACE: Duration = Duration::from_millis(250);
const PREWARM_RETRY_INTERVAL: Duration = Duration::from_secs(3);

enum LiveCommand {
    Warm(Option<LiveConfig>),
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

    pub fn warm(&self, config: Option<LiveConfig>) {
        let _ = self.tx.try_send(LiveCommand::Warm(config));
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
    start_sent: bool,
    end_requested: bool,
    end_sent: bool,
    end_sent_at: Option<Instant>,
    completion_seen_at: Option<Instant>,
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
            start_sent: false,
            end_requested: false,
            end_sent: false,
            end_sent_at: None,
            completion_seen_at: None,
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
    let mut warm_config: Option<LiveConfig> = None;
    let mut last_prewarm_attempt: Option<Instant> = None;
    let mut maintenance_tick = tokio::time::interval(Duration::from_millis(25));

    loop {
        pump_front(&mut session, &mut queue, &websocket_url, timeout).await;

        if queue.is_empty() {
            if let Some(target) = warm_config.as_ref() {
                if !target.key.is_empty() {
                    let needs_refresh = session
                        .as_ref()
                        .map(|open| {
                            open.config != *target
                                || open.opened_at.elapsed() >= SESSION_REFRESH_AFTER
                        })
                        .unwrap_or(true);

                    if needs_refresh {
                        let can_attempt = last_prewarm_attempt
                            .map(|t| t.elapsed() >= PREWARM_RETRY_INTERVAL)
                            .unwrap_or(true);

                        if can_attempt {
                            session = None;
                            last_prewarm_attempt = Some(Instant::now());
                            if let Ok(open) = connect_live(&websocket_url, target, timeout).await {
                                session = Some(open);
                                last_prewarm_attempt = None;
                            }
                        }
                    }
                }
            }
        }

        if let Some(open) = session.as_mut() {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    handle_command(
                        command,
                        &mut queue,
                        &mut warm_config,
                        &mut session,
                        &mut last_prewarm_attempt,
                    );
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
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    handle_command(
                        command,
                        &mut queue,
                        &mut warm_config,
                        &mut session,
                        &mut last_prewarm_attempt,
                    );
                }
                _ = maintenance_tick.tick() => {}
            }
        }
    }
}

fn handle_command(
    command: LiveCommand,
    queue: &mut VecDeque<Utterance>,
    warm_config: &mut Option<LiveConfig>,
    session: &mut Option<LiveSession>,
    last_prewarm_attempt: &mut Option<Instant>,
) {
    match command {
        LiveCommand::Warm(config) => {
            if *warm_config != config {
                *warm_config = config;
                *last_prewarm_attempt = None;
                if queue.is_empty() {
                    if let Some(open) = session {
                        if Some(&open.config) != warm_config.as_ref() {
                            *session = None;
                        }
                    }
                }
            }
        }
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
                // Append synthetic trailing silence (200 ms at 16 kHz = 3200 samples)
                // so Gemini Live streaming ASR decoder has lookahead acoustic context
                // to finalize trailing phonemes and avoid clipping the last word.
                if !utterance.pcm.is_empty() {
                    utterance.pcm.extend(vec![0i16; 3200]);
                }
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
            if !front.start_sent {
                front.start_sent = true;
                Some(activity_start_message())
            } else if front.sent_samples + LIVE_FRAME_SAMPLES <= front.pcm.len() {
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
                    front.completion_seen_at = None;
                    Some(activity_end_message())
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
            let mut completed_text = None;
            let signal = if let Some(front) = queue.front_mut() {
                let signal = front.transcript.ingest(&value);
                let current_text = front.transcript.text();
                if !current_text.is_empty() {
                    if let Some(ref tx) = front.stream_tx {
                        let _ = tx.send(current_text);
                    }
                }
                match signal {
                    ServerSignal::TurnComplete => {
                        if front.end_sent {
                            completed_text = Some(front.transcript.text());
                        }
                    }
                    ServerSignal::GenerationComplete => {
                        if front.end_sent {
                            if !front.transcript.text().is_empty() {
                                completed_text = Some(front.transcript.text());
                            } else {
                                front.completion_seen_at = Some(Instant::now());
                            }
                        }
                    }
                    ServerSignal::Continue => {
                        if front.end_sent
                            && front.completion_seen_at.is_some()
                            && !front.transcript.text().is_empty()
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
        front.start_sent = false;
        front.end_sent = false;
        front.end_sent_at = None;
        front.completion_seen_at = None;
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
