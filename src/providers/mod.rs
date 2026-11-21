pub mod anthropic;
pub mod mock;
pub mod openai;
pub mod sse;

use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::config::{ProviderKind, Resolved};
use crate::protocol::{Message, StopReason};

/// Tool declaration sent to the model.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// One model call request, provider-independent.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub system: Vec<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub thinking_budget: Option<u32>,
}

/// Normalized streaming event produced by every provider.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    TextDelta(String),
    ThinkingDelta(String),
    /// Anthropic extended-thinking signature, required when replaying history.
    ThinkingSignature(String),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallArgsDelta {
        index: usize,
        json: String,
    },
    Usage {
        input: u64,
        output: u64,
    },
    MessageStop {
        stop_reason: StopReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Config,
    Auth,
    RateLimit,
    Network,
    Protocol,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub status: Option<u16>,
}

impl ProviderError {
    pub fn config(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::Config,
            message: message.into(),
            status: None,
        }
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::Auth,
            message: message.into(),
            status: None,
        }
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::Network,
            message: message.into(),
            status: None,
        }
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self {
            kind: ProviderErrorKind::Protocol,
            message: message.into(),
            status: None,
        }
    }

    pub fn from_status(status: u16, body: &str) -> Self {
        let detail = extract_error_message(body);
        let kind = match status {
            401 | 403 => ProviderErrorKind::Auth,
            429 => ProviderErrorKind::RateLimit,
            400 | 404 | 422 => ProviderErrorKind::Config,
            _ => ProviderErrorKind::Protocol,
        };
        Self {
            kind,
            message: format!("HTTP {status}: {detail}"),
            status: Some(status),
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self.kind, ProviderErrorKind::RateLimit)
            || self.status.map(|s| s >= 500).unwrap_or(false)
    }
}

fn extract_error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return message.to_string();
        }
        if let Some(message) = value.get("message").and_then(|m| m.as_str()) {
            return message.to_string();
        }
    }
    let trimmed = body.trim();
    if trimmed.len() > 300 {
        format!("{}…", &trimmed[..300])
    } else if trimmed.is_empty() {
        "empty response body".to_string()
    } else {
        trimmed.to_string()
    }
}

impl From<ProviderError> for crate::error::AppError {
    fn from(value: ProviderError) -> Self {
        match value.kind {
            ProviderErrorKind::Config | ProviderErrorKind::Auth => {
                crate::error::AppError::Config(value.message)
            }
            _ => crate::error::AppError::Runtime(value.message),
        }
    }
}

pub type EventStream = Pin<Box<dyn Stream<Item = Result<ProviderEvent, ProviderError>> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    async fn stream(
        &self,
        req: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<EventStream, ProviderError>;
}

pub fn http_client() -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| ProviderError::network(format!("cannot build HTTP client: {e}")))
}

pub fn build(resolved: &Resolved) -> Result<Box<dyn Provider>, ProviderError> {
    match resolved.kind {
        ProviderKind::Anthropic => Ok(Box::new(anthropic::AnthropicProvider::new(resolved.clone())?)),
        ProviderKind::OpenAi => Ok(Box::new(openai::OpenAiProvider::new(resolved.clone())?)),
    }
}

/// Join a base URL and a path without doubling or dropping slashes.
pub fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_urls_cleanly() {
        assert_eq!(
            join_url("https://api.anthropic.com/", "/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            join_url("http://localhost:8000/v1", "chat/completions"),
            "http://localhost:8000/v1/chat/completions"
        );
    }

    #[test]
    fn maps_status_to_kind() {
        assert_eq!(ProviderError::from_status(401, "{}").kind, ProviderErrorKind::Auth);
        assert_eq!(ProviderError::from_status(429, "{}").kind, ProviderErrorKind::RateLimit);
        assert!(ProviderError::from_status(429, "{}").is_retryable());
        assert!(!ProviderError::from_status(400, "{}").is_retryable());
    }
}
