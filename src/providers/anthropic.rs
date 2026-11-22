use async_trait::async_trait;
use serde_json::{Value, json};

use super::{CompletionRequest, Provider, ProviderError, join_url};
use crate::config::Resolved;
use crate::protocol::{Block, Message};

mod stream;
#[cfg(test)]
mod tests;

pub use stream::normalize_event;

pub struct AnthropicProvider {
    client: reqwest::Client,
    resolved: Resolved,
}

impl AnthropicProvider {
    pub fn new(resolved: Resolved) -> Result<Self, ProviderError> {
        Ok(Self {
            client: super::http_client()?,
            resolved,
        })
    }
}

fn messages_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/v1") {
        join_url(base, "messages")
    } else {
        join_url(base, "v1/messages")
    }
}

pub fn build_body(req: &CompletionRequest) -> Value {
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "messages": messages_to_wire(&req.messages),
    });

    if !req.system.is_empty() {
        body["system"] = json!([{
            "type": "text",
            "text": req.system.join("\n\n"),
        }]);
    }

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }

    if let Some(temperature) = req.temperature
        && req.thinking_budget.unwrap_or(0) == 0 {
            body["temperature"] = json!(temperature);
        }

    if let Some(budget) = req.thinking_budget
        && budget > 0 {
            body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
        }

    body
}

pub fn messages_to_wire(messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    for message in messages {
        match message {
            Message::User { content } => {
                out.push(json!({"role": "user", "content": content}));
            }
            Message::Assistant { content } => {
                let mut blocks: Vec<Value> = Vec::new();
                for block in content {
                    match block {
                        Block::Text { text } => {
                            if !text.is_empty() {
                                blocks.push(json!({"type": "text", "text": text}));
                            }
                        }
                        Block::Thinking { text, signature } => {
                            if !text.is_empty() {
                                let mut value = json!({"type": "thinking", "thinking": text});
                                if let Some(signature) = signature {
                                    value["signature"] = json!(signature);
                                }
                                blocks.push(value);
                            }
                        }
                        Block::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": arguments,
                            }));
                        }
                    }
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": " "}));
                }
                out.push(json!({"role": "assistant", "content": blocks}));
            }
            Message::ToolResults { results } => {
                let blocks: Vec<Value> = results
                    .iter()
                    .map(|result| {
                        json!({
                            "type": "tool_result",
                            "tool_use_id": result.tool_call_id,
                            "content": result.content,
                            "is_error": result.is_error,
                        })
                    })
                    .collect();
                out.push(json!({"role": "user", "content": blocks}));
            }
        }
    }
    out
}

/// Anthropic requires the `stream` flag at request time; this trait impl lives
/// in `stream.rs` to keep request shaping and wire parsing separate.
#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.resolved.name
    }

    fn model(&self) -> &str {
        &self.resolved.model
    }

    async fn stream(
        &self,
        req: CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<super::EventStream, ProviderError> {
        stream::open(self, req, cancel).await
    }
}
