use base64::Engine;
use tokio_tungstenite::tungstenite;

use super::GeminiError;

pub const PROD_BASE_URL: &str = "https://generativelanguage.googleapis.com";
pub const PROD_WS_URL: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

pub fn setup_message(model: &str, vocabulary: &[String], smart: bool) -> serde_json::Value {
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
            "realtimeInputConfig": {
                "automaticActivityDetection": {
                    "disabled": true,
                }
            }
        }
    })
}

pub fn activity_start_message() -> serde_json::Value {
    serde_json::json!({
        "realtimeInput": {
            "activityStart": {}
        }
    })
}

pub fn audio_message(samples: &[i16]) -> serde_json::Value {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    serde_json::json!({
        "realtimeInput": {
            "audio": {
                "data": base64::engine::general_purpose::URL_SAFE.encode(pcm),
                "mimeType": "audio/pcm;rate=16000",
            }
        }
    })
}

pub fn activity_end_message() -> serde_json::Value {
    serde_json::json!({
        "realtimeInput": {
            "activityEnd": {}
        }
    })
}

pub fn websocket_error(error: tungstenite::Error) -> GeminiError {
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

pub fn decode_server_message(
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
