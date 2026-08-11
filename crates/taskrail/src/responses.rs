use crate::core::ResponsesSpec;
use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{env, fmt, time::Duration};
use tokio::sync::watch;

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_MODEL: &str = "gpt-5";
pub const DEFAULT_API_KEY_ENV: &str = "OPENAI_API_KEY";
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct ResponsesConfig {
    base_url: String,
    model: String,
    api_key: String,
    store: bool,
    timeout: Duration,
}

impl fmt::Debug for ResponsesConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponsesConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"[REDACTED]")
            .field("store", &self.store)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl ResponsesConfig {
    pub fn from_spec(spec: &ResponsesSpec, timeout_seconds: u64) -> Result<Self> {
        let base_url = spec
            .base_url
            .clone()
            .or_else(|| env::var("TASKRAIL_RESPONSES_BASE_URL").ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let model = spec
            .model
            .clone()
            .or_else(|| env::var("TASKRAIL_RESPONSES_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        Self::from_env(
            base_url,
            model,
            &spec.api_key_env,
            spec.store,
            timeout_seconds,
        )
    }

    pub fn from_env(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key_env: &str,
        store: bool,
        timeout_seconds: u64,
    ) -> Result<Self> {
        let api_key = env::var(api_key_env)
            .with_context(|| format!("environment variable {api_key_env} is not set"))?;
        if api_key.trim().is_empty() {
            anyhow::bail!("environment variable {api_key_env} is empty");
        }
        Self::new(base_url, model, api_key, store, timeout_seconds)
    }

    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        store: bool,
        timeout_seconds: u64,
    ) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
            anyhow::bail!("Responses API base URL must use http:// or https://");
        }
        let model = model.into();
        if model.trim().is_empty() {
            anyhow::bail!("Responses API model must not be empty");
        }
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            anyhow::bail!("Responses API API key must not be empty");
        }
        if timeout_seconds == 0 {
            anyhow::bail!("Responses API timeout must be greater than zero");
        }
        Ok(Self {
            base_url,
            model,
            api_key,
            store,
            timeout: Duration::from_secs(timeout_seconds),
        })
    }

    pub fn endpoint(&self) -> String {
        format!("{}/responses", self.base_url)
    }

    pub async fn execute(&self, input: impl Into<String>) -> Result<ResponsesResult> {
        let (_sender, cancellation) = watch::channel(false);
        self.execute_with_cancellation(input, cancellation).await
    }

    pub async fn execute_with_cancellation(
        &self,
        input: impl Into<String>,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<ResponsesResult> {
        if *cancellation.borrow() {
            anyhow::bail!("Responses API request cancelled before start");
        }
        let request = self.send(input.into());
        tokio::pin!(request);
        tokio::select! {
            result = &mut request => result,
            changed = cancellation.changed() => {
                if changed.is_ok() && *cancellation.borrow() {
                    anyhow::bail!("Responses API request cancelled by supervisor");
                }
                anyhow::bail!("Responses API cancellation channel closed");
            }
        }
    }

    async fn send(&self, input: String) -> Result<ResponsesResult> {
        let client = Client::builder()
            .timeout(self.timeout)
            .build()
            .context("build Responses API client")?;
        let response = client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&ResponsesRequest {
                model: &self.model,
                input,
                store: self.store,
            })
            .send()
            .await
            .context("send Responses API request")?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("read Responses API response")?;
        if !status.is_success() {
            return Err(api_error(status, &body, &self.api_key));
        }
        let parsed: ApiResponse =
            serde_json::from_slice(&body).context("decode Responses API response as JSON")?;
        let output_text = parsed
            .output_text
            .or_else(|| extract_output_text(&parsed.output))
            .map(|text| bounded_text(&text))
            .filter(|text| !text.is_empty())
            .context("Responses API response did not contain assistant output text")?;
        Ok(ResponsesResult {
            id: parsed.id,
            model: parsed.model,
            output_text,
            usage: parsed.usage,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    input: String,
    store: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesResult {
    pub id: Option<String>,
    pub model: Option<String>,
    pub output_text: String,
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    id: Option<String>,
    model: Option<String>,
    output_text: Option<String>,
    output: Option<Vec<OutputItem>>,
    usage: Option<UsageSummary>,
}

#[derive(Debug, Deserialize)]
struct OutputItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    content: Option<Vec<OutputPart>>,
}

#[derive(Debug, Deserialize)]
struct OutputPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    r#type: Option<String>,
    message: Option<String>,
}

fn extract_output_text(output: &Option<Vec<OutputItem>>) -> Option<String> {
    let mut text = String::new();
    for item in output.as_ref()? {
        if item.item_type.as_deref() != Some("message") {
            continue;
        }
        for part in item.content.as_deref().unwrap_or_default() {
            if part.part_type.as_deref() == Some("output_text") {
                if let Some(value) = &part.text {
                    text.push_str(value);
                }
            }
        }
    }
    (!text.is_empty()).then_some(text)
}

fn bounded_text(value: &str) -> String {
    if value.len() <= MAX_OUTPUT_BYTES {
        return value.to_owned();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = value[..end].to_owned();
    output.push_str("\n[taskrail: output truncated at 1 MiB]\n");
    output
}

fn api_error(status: StatusCode, body: &[u8], secret: &str) -> anyhow::Error {
    let parsed = serde_json::from_slice::<ApiErrorEnvelope>(body).ok();
    let kind = parsed
        .as_ref()
        .and_then(|value| value.error.as_ref())
        .and_then(|value| value.r#type.as_deref())
        .map(str::to_owned)
        .unwrap_or_else(|| "api_error".to_owned());
    let message = parsed
        .and_then(|value| value.error)
        .and_then(|value| value.message)
        .unwrap_or_else(|| "request failed".to_owned());
    let message = message.replace(secret, "[REDACTED]");
    anyhow::anyhow!("Responses API returned HTTP {status} ({kind}): {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_assistant_output_text_and_skips_reasoning() {
        let response: ApiResponse = serde_json::from_value(serde_json::json!({
            "id": "resp_test",
            "model": "test-model",
            "output": [
                {"type": "reasoning", "content": [{"type": "reasoning_text", "text": "private"}]},
                {"type": "message", "content": [{"type": "output_text", "text": "hello"}]}
            ]
        }))
        .unwrap();
        assert_eq!(
            extract_output_text(&response.output).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn config_debug_redacts_the_api_key() {
        let config = ResponsesConfig::new(
            "https://example.test/v1",
            "test-model",
            "secret-value",
            false,
            10,
        )
        .unwrap();
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret-value"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn validates_base_url_and_timeout() {
        assert!(ResponsesConfig::new("example.test", "model", "key", false, 10).is_err());
        assert!(ResponsesConfig::new("https://example.test", "model", "key", false, 0).is_err());
    }

    #[test]
    fn bounds_utf8_output_without_splitting_a_codepoint() {
        let output = bounded_text(&"界".repeat(MAX_OUTPUT_BYTES));
        assert!(output.is_char_boundary(output.len()));
        assert!(output.ends_with("output truncated at 1 MiB]\n"));
    }
}
