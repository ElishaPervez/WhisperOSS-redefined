use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use super::client::GeminiClient;
use super::live::GeminiLive;
use super::protocol::{audio_message, setup_message};
use super::transcript_buffer::{ServerSignal, TranscriptBuffer};
use super::{GeminiError, LiveConfig, PROD_WS_URL};

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
    assert_eq!(
        message["setup"]["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
        true
    );
}

#[test]
fn default_verbatim_setup_matches_minimal_live_sdk_request() {
    let message = setup_message("gemini-3.5-transcribe-live", &[], false);

    assert_eq!(
        message["setup"]["inputAudioTranscription"],
        serde_json::json!({})
    );
    assert_eq!(
        message["setup"]["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
        true
    );
}

#[test]
fn audio_frame_is_little_endian_pcm_at_16khz() {
    let message = audio_message(&[0, 1, -1]);

    assert_eq!(
        message["realtimeInput"]["audio"]["mimeType"],
        "audio/pcm;rate=16000"
    );
    assert_eq!(message["realtimeInput"]["audio"]["data"], "AAABAP__");
}

#[test]
fn final_transcript_supersedes_interim_text() {
    let mut transcript = TranscriptBuffer::default();
    assert_eq!(
        transcript.ingest(&serde_json::json!({
            "serverContent": { "interimInputTranscription": { "text": "streaming phrase" } }
        })),
        ServerSignal::Continue
    );
    assert_eq!(transcript.text(), "streaming phrase");

    transcript.ingest(&serde_json::json!({
        "serverContent": {
            "inputTranscription": { "text": "finalized complete sentence" },
            "turnComplete": true
        }
    }));
    assert_eq!(transcript.text(), "finalized complete sentence");
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
            let mut activity_started = false;
            loop {
                let value: serde_json::Value = serde_json::from_str(
                    websocket.read().unwrap().into_text().unwrap().as_str(),
                )
                .unwrap();
                if value["realtimeInput"]["activityStart"].is_object() {
                    activity_started = true;
                }
                if value["realtimeInput"]["audio"].is_object() {
                    audio_frames += 1;
                }
                if value["realtimeInput"]["activityEnd"].is_object() {
                    break;
                }
            }
            assert!(activity_started);
            assert!(audio_frames >= 1);
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
        let first_val: serde_json::Value =
            serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                .unwrap();
        assert!(first_val["realtimeInput"]["activityStart"].is_object());
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
            if value["realtimeInput"]["activityEnd"].is_object() {
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
            if value["realtimeInput"]["activityEnd"].is_object() {
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
fn live_manager_handles_speech_pauses_without_splitting_session() {
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
        let _ = websocket.read().unwrap(); // activityStart
        let _ = websocket.read().unwrap(); // first audio chunk
        websocket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"serverContent":{"interimInputTranscription":{"text":"speaking part one"}}}"#.into(),
            ))
            .unwrap();
        // Server receives pause audio and subsequent speech until activityEnd
        loop {
            let msg = websocket.read().unwrap();
            if let Ok(text) = msg.into_text() {
                let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                if val["realtimeInput"]["activityEnd"].is_object() {
                    break;
                }
            }
        }
        websocket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::json!({
                    "serverContent": {
                        "inputTranscription": { "text": "speaking part one and then continuing seamlessly" },
                        "turnComplete": true
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
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
    // Wait long enough for interim processing while still recording
    std::thread::sleep(Duration::from_millis(150));
    // Push 2 more seconds of silence (pause)
    sink.push(vec![0; 1_600], 16_000);
    // Push subsequent speech
    sink.push(vec![700; 1_600], 16_000);
    // Release key (finish)
    let result = live
        .finish()
        .unwrap()
        .recv_timeout(Duration::from_millis(800))
        .unwrap()
        .unwrap();

    assert_eq!(result, "speaking part one and then continuing seamlessly");
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

#[test]
fn live_manager_sends_activity_start_and_end_wrapping_speech() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut websocket = tokio_tungstenite::tungstenite::accept(tcp).unwrap();
        let setup: serde_json::Value =
            serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                .unwrap();
        assert_eq!(
            setup["setup"]["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
            true
        );
        websocket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"setupComplete":{}}"#.into(),
            ))
            .unwrap();

        // 1. ActivityStart
        let start_msg: serde_json::Value =
            serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                .unwrap();
        assert!(start_msg["realtimeInput"]["activityStart"].is_object());

        // 2. Audio chunks until activityEnd
        let mut audio_chunks = 0;
        loop {
            let msg: serde_json::Value =
                serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                    .unwrap();
            if msg["realtimeInput"]["audio"].is_object() {
                audio_chunks += 1;
            }
            if msg["realtimeInput"]["activityEnd"].is_object() {
                break;
            }
        }
        assert!(audio_chunks >= 1);

        // Respond with full finalized transcript
        websocket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::json!({
                    "serverContent": {
                        "inputTranscription": {
                            "text": "Okay, I am now testing whether after I stop speaking and then I start speaking again it's going to be fixed."
                        },
                        "turnComplete": true
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
    });

    let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_secs(2));
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
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();

    assert_eq!(
        result,
        "Okay, I am now testing whether after I stop speaking and then I start speaking again it's going to be fixed."
    );
    server.join().unwrap();
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
    for chunk in samples.chunks(640) {
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

#[test]
fn live_manager_prewarms_connection_before_recording() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut websocket = tokio_tungstenite::tungstenite::accept(tcp).unwrap();
        // 1. Should receive setup message upon pre-warming (BEFORE any begin/start call)
        let setup: serde_json::Value =
            serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                .unwrap();
        assert_eq!(setup["setup"]["model"], "models/gemini-3.5-transcribe-live");
        websocket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"setupComplete":{}}"#.into(),
            ))
            .unwrap();

        // 2. Now when begin() is called, the first message read MUST be activityStart
        let start_msg: serde_json::Value =
            serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                .unwrap();
        assert!(start_msg["realtimeInput"]["activityStart"].is_object());

        let mut audio_chunks = 0;
        loop {
            let msg: serde_json::Value =
                serde_json::from_str(websocket.read().unwrap().into_text().unwrap().as_str())
                    .unwrap();
            if msg["realtimeInput"]["audio"].is_object() {
                audio_chunks += 1;
            }
            if msg["realtimeInput"]["activityEnd"].is_object() {
                break;
            }
        }
        assert!(audio_chunks >= 1);

        websocket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::json!({
                    "serverContent": {
                        "inputTranscription": { "text": "prewarmed instant text" },
                        "turnComplete": true
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
    });

    let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_secs(2));
    let config = LiveConfig {
        key: "test-key".into(),
        model: "gemini-3.5-transcribe-live".into(),
        vocabulary: Vec::new(),
        smart: false,
    };

    // Pre-warm the connection
    live.warm(Some(config.clone()));

    // Give a brief moment for the background prewarm handshake to complete
    std::thread::sleep(Duration::from_millis(100));

    // Now start recording - it should use the pre-warmed connection directly
    let sink = live.begin(config, None).unwrap();
    sink.push(vec![700; 1_600], 16_000);
    let result = live
        .finish()
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();

    assert_eq!(result, "prewarmed instant text");
    server.join().unwrap();
}

#[test]
fn live_manager_prewarm_reconnects_when_config_changes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        // First connection with config 1 (smart: false)
        let (tcp1, _) = listener.accept().unwrap();
        let mut ws1 = tokio_tungstenite::tungstenite::accept(tcp1).unwrap();
        let setup1: serde_json::Value =
            serde_json::from_str(ws1.read().unwrap().into_text().unwrap().as_str()).unwrap();
        assert_eq!(
            setup1["setup"]["inputAudioTranscription"]["mode"],
            serde_json::Value::Null
        );
        ws1.send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"setupComplete":{}}"#.into(),
        ))
        .unwrap();

        // Config changes to smart: true -> closes first connection and opens second
        let (tcp2, _) = listener.accept().unwrap();
        let mut ws2 = tokio_tungstenite::tungstenite::accept(tcp2).unwrap();
        let setup2: serde_json::Value =
            serde_json::from_str(ws2.read().unwrap().into_text().unwrap().as_str()).unwrap();
        assert_eq!(
            setup2["setup"]["inputAudioTranscription"]["mode"],
            "SMART"
        );
        ws2.send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"setupComplete":{}}"#.into(),
        ))
        .unwrap();

        // Recording begins on ws2
        let start_msg: serde_json::Value =
            serde_json::from_str(ws2.read().unwrap().into_text().unwrap().as_str()).unwrap();
        assert!(start_msg["realtimeInput"]["activityStart"].is_object());
        let mut audio_chunks = 0;
        loop {
            let msg: serde_json::Value =
                serde_json::from_str(ws2.read().unwrap().into_text().unwrap().as_str())
                    .unwrap();
            if msg["realtimeInput"]["audio"].is_object() {
                audio_chunks += 1;
            }
            if msg["realtimeInput"]["activityEnd"].is_object() {
                break;
            }
        }
        assert!(audio_chunks >= 1);
        ws2.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({
                "serverContent": {
                    "inputTranscription": { "text": "smart formatted text" },
                    "turnComplete": true
                }
            })
            .to_string()
            .into(),
        ))
        .unwrap();
    });

    let live = GeminiLive::spawn(format!("ws://{address}/"), Duration::from_secs(2));
    let config1 = LiveConfig {
        key: "test-key".into(),
        model: "gemini-3.5-transcribe-live".into(),
        vocabulary: Vec::new(),
        smart: false,
    };

    live.warm(Some(config1));
    std::thread::sleep(Duration::from_millis(80));

    let config2 = LiveConfig {
        key: "test-key".into(),
        model: "gemini-3.5-transcribe-live".into(),
        vocabulary: Vec::new(),
        smart: true,
    };
    live.warm(Some(config2.clone()));
    std::thread::sleep(Duration::from_millis(80));

    let sink = live.begin(config2, None).unwrap();
    sink.push(vec![700; 1_600], 16_000);
    let result = live
        .finish()
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();

    assert_eq!(result, "smart formatted text");
    server.join().unwrap();
}
