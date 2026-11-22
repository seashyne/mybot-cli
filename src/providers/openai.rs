use std::sync::OnceLock;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{CompletionRequest, EventStream, Provider, ProviderError, join_url};
use crate::config::Resolved;
use crate::protocol::{Block, Message};

mod stream;
#[cfg(test)]
mod tests;

pub use stream::normalize_event;

pub struct OpenAiProvider {
    client: reqwest::Client,
    resolved: Resolved,
    resolved_model: OnceLock<String>,
}

impl OpenAiProvider {
    pub fn new(resolved: Resolved) -> Result<Self, ProviderError> {
        Ok(Self {
            client: super::http_client()?,
            resolved,
            resolved_model: OnceLock::new(),
        })
    }

    /// When the configured model is `auto` (or empty), ask the server which
    /// model it serves. LM Studio, Ollama, and llama.cpp expose `/v1/models`.
    async fn resolve_model(&self, requested: &str) -> Result<String, ProviderError> {
        let requested = requested.trim();
        if !requested.is_empty() && !requested.eq_ignore_ascii_case("auto") {
            return Ok(requested.to_string());
        }

        let url = models_url(&self.resolved.base_url);
        let mut request = self.client.get(&url);
        if !self.resolved.api_key.is_empty() {
            request = request.bearer_auth(&self.resolved.api_key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ProviderError::network(format!("cannot list models at {url}: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = super::retry_after_header(&response);
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::from_status(status.as_u16(), &text)
                .with_retry_after(retry_after));
        }

        let value: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::protocol(format!("invalid model list: {e}")))?;
        let model = pick_model(&value).ok_or_else(|| {
            ProviderError::config(format!(
                "no chat model available at {url}; load a chat model in your local server or pass -m <model>"
            ))
        })?;

        let _ = self.resolved_model.set(model.clone());
        Ok(model)
    }
}

/// Pick the first non-embedding model from an OpenAI-style `/v1/models` list.
fn pick_model(value: &Value) -> Option<String> {
    value
        .get("data")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .find(|id| !id.is_empty() && !looks_like_embedding(id))
        .map(str::to_string)
}

fn looks_like_embedding(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    id.contains("embed") || id.contains("bge") || id.starts_with("e5-")
}

fn models_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if let Some(root) = base.strip_suffix("/chat/completions") {
        join_url(root, "models")
    } else if base.ends_with("/v1") {
        join_url(base, "models")
    } else {
        join_url(base, "v1/models")
    }
}

fn completions_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") {
        join_url(base, "chat/completions")
    } else {
        join_url(base, "v1/chat/completions")
    }
}

/// Newer OpenAI reasoning models reject `max_tokens` in favour of
/// `max_completion_tokens`.
fn max_tokens_field(model: &str) -> &'static str {
    let model = model.to_ascii_lowercase();
    if model.starts_with("gpt-5")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        "max_completion_tokens"
    } else {
        "max_tokens"
    }
}

pub fn build_body(req: &CompletionRequest) -> Value {
    let mut body = json!({
        "model": req.model,
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": messages_to_wire(&req.system, &req.messages),
    });
    body[max_tokens_field(&req.model)] = json!(req.max_tokens);

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }

    if let Some(temperature) = req.temperature
        && req.thinking_budget.unwrap_or(0) == 0 {
            body["temperature"] = json!(temperature);
        }

    body
}

pub fn messages_to_wire(system: &[String], messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    for part in system {
        if !part.trim().is_empty() {
            out.push(json!({"role": "system", "content": part}));
        }
    }

    for message in messages {
        match message {
            Message::User { content } => out.push(json!({"role": "user", "content": content})),
            Message::Assistant { content } => {
                let text = content
                    .iter()
                    .filter_map(|block| block.text())
                    .collect::<Vec<_>>()
                    .join("");
                let tool_calls: Vec<Value> = content
                    .iter()
                    .filter_map(|block| match block {
                        Block::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments.to_string(),
                            }
                        })),
                        _ => None,
                    })
                    .collect();

                let mut value = json!({
                    "role": "assistant",
                    "content": if text.is_empty() { Value::Null } else { json!(text) },
                });
                if !tool_calls.is_empty() {
                    value["tool_calls"] = Value::Array(tool_calls);
                }
                out.push(value);
            }
            Message::ToolResults { results } => {
                for result in results {
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": result.tool_call_id,
                        "content": result.content,
                    }));
                }
            }
        }
    }
    out
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.resolved.name
    }

    fn model(&self) -> &str {
        self.resolved_model
            .get()
            .map(String::as_str)
            .unwrap_or(&self.resolved.model)
    }

    async fn stream(
        &self,
        req: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<EventStream, ProviderError> {
        stream::open(self, req, cancel).await
    }
}
