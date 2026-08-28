use std::time::Duration;

use super::GeminiError;

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
