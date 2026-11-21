use std::sync::OnceLock;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::sse::{SseDecoder, SseEvent};
use super::{CompletionRequest, EventStream, Provider, ProviderError, ProviderEvent, join_url};
use crate::config::Resolved;
use crate::protocol::{Block, Message, StopReason};

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
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::from_status(status.as_u16(), &text));
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
        let url = completions_url(&self.resolved.base_url);
        let mut req = req;
        req.model = self.resolve_model(&req.model).await?;
        let body = build_body(&req);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if !self.resolved.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", self.resolved.api_key))
                    .map_err(|_| ProviderError::config("API key contains invalid header characters"))?,
            );
        }
        for (key, value) in &self.resolved.headers {
            let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| ProviderError::config(format!("invalid header name '{key}'")))?;
            let value = HeaderValue::from_str(value)
                .map_err(|_| ProviderError::config(format!("invalid header value for '{key}'")))?;
            headers.insert(name, value);
        }

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::network(format!("request to {url} failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::from_status(status.as_u16(), &text));
        }

        let mut byte_stream = response.bytes_stream();
        let stream = async_stream::stream! {
            let mut decoder = SseDecoder::new();
            let mut stop_sent = false;

            loop {
                let item = tokio::select! {
                    _ = cancel.cancelled() => {
                        yield Ok(ProviderEvent::MessageStop { stop_reason: StopReason::Aborted });
                        return;
                    }
                    item = byte_stream.next() => item,
                };

                let Some(item) = item else { break };
                match item {
                    Err(error) => {
                        yield Err(ProviderError::network(format!("stream error: {error}")));
                        return;
                    }
                    Ok(bytes) => {
                        let mut done = false;
                        for event in decoder.push(&bytes) {
                            if event.data.trim() == "[DONE]" {
                                done = true;
                                break;
                            }
                            for normalized in normalize_event(&event) {
                                if matches!(&normalized, Ok(ProviderEvent::MessageStop { .. })) {
                                    stop_sent = true;
                                }
                                yield normalized;
                            }
                        }
                        if done {
                            break;
                        }
                    }
                }
            }

            if let Some(event) = decoder.finish() {
                for normalized in normalize_event(&event) {
                    if matches!(&normalized, Ok(ProviderEvent::MessageStop { .. })) {
                        stop_sent = true;
                    }
                    yield normalized;
                }
            }

            if !stop_sent {
                yield Ok(ProviderEvent::MessageStop { stop_reason: StopReason::EndTurn });
            }
        };

        Ok(Box::pin(stream))
    }
}

pub fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "stop" => StopReason::EndTurn,
        "content_filter" => StopReason::Error,
        _ => StopReason::EndTurn,
    }
}

pub fn normalize_event(event: &SseEvent) -> Vec<Result<ProviderEvent, ProviderError>> {
    let mut out = Vec::new();
    let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
        if event.data.trim().is_empty() {
            return out;
        }
        out.push(Err(ProviderError::protocol(format!(
            "invalid JSON in SSE chunk: {}",
            event.data.chars().take(200).collect::<String>()
        ))));
        return out;
    };

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("provider stream error");
        out.push(Err(ProviderError::protocol(message.to_string())));
        return out;
    }

    if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
        out.push(Ok(ProviderEvent::Usage {
            input: usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output: usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        }));
    }

    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    else {
        return out;
    };

    if let Some(delta) = choice.get("delta") {
        if let Some(text) = delta.get("content").and_then(Value::as_str)
            && !text.is_empty() {
                out.push(Ok(ProviderEvent::TextDelta(text.to_string())));
            }
        for key in ["reasoning_content", "reasoning", "reasoning_text"] {
            if let Some(text) = delta.get(key).and_then(Value::as_str)
                && !text.is_empty() {
                    out.push(Ok(ProviderEvent::ThinkingDelta(text.to_string())));
                }
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
                let function = call.get("function").cloned().unwrap_or(Value::Null);
                let name = function.get("name").and_then(Value::as_str).unwrap_or_default();
                if !id.is_empty() || !name.is_empty() {
                    out.push(Ok(ProviderEvent::ToolCallStart {
                        index,
                        id: id.to_string(),
                        name: name.to_string(),
                    }));
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str)
                    && !arguments.is_empty() {
                        out.push(Ok(ProviderEvent::ToolCallArgsDelta {
                            index,
                            json: arguments.to_string(),
                        }));
                    }
            }
        }
    }

    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        out.push(Ok(ProviderEvent::MessageStop {
            stop_reason: map_finish_reason(reason),
        }));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> CompletionRequest {
        CompletionRequest {
            model: "gpt-4o-mini".into(),
            system: vec!["sys".into()],
            messages: vec![Message::user("hi")],
            tools: vec![super::super::ToolSpec {
                name: "read".into(),
                description: "read".into(),
                input_schema: json!({"type": "object"}),
            }],
            max_tokens: 100,
            temperature: None,
            thinking_budget: None,
        }
    }

    #[test]
    fn body_uses_system_message_and_function_tools() {
        let body = build_body(&req());
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["max_tokens"], 100);
    }

    #[test]
    fn reasoning_models_use_max_completion_tokens() {
        let mut request = req();
        request.model = "gpt-5-mini".into();
        let body = build_body(&request);
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["max_completion_tokens"], 100);
    }

    #[test]
    fn tool_results_become_tool_messages() {
        let messages = vec![Message::ToolResults {
            results: vec![crate::protocol::ToolResult {
                tool_call_id: "c1".into(),
                tool_name: "read".into(),
                content: "ok".into(),
                is_error: false,
            }],
        }];
        let wire = messages_to_wire(&[], &messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["role"], "tool");
        assert_eq!(wire[0]["tool_call_id"], "c1");
    }

    #[test]
    fn accumulates_tool_call_deltas() {
        let start = SseEvent {
            event: None,
            data: json!({
                "choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c1", "function": {"name": "read", "arguments": ""}}]}}]
            })
            .to_string(),
        };
        let events = normalize_event(&start);
        assert!(matches!(
            &events[0],
            Ok(ProviderEvent::ToolCallStart { id, name, .. }) if id == "c1" && name == "read"
        ));

        let args = SseEvent {
            event: None,
            data: json!({
                "choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{\"path\":\"a\"}"}}]}}]
            })
            .to_string(),
        };
        let events = normalize_event(&args);
        assert!(matches!(
            &events[0],
            Ok(ProviderEvent::ToolCallArgsDelta { json, .. }) if json.contains("path")
        ));
    }

    #[test]
    fn auto_model_skips_embedding_models() {
        let value = json!({
            "data": [
                {"id": "text-embedding-nomic-embed-text-v1.5"},
                {"id": "qwen2.5-7b-instruct"}
            ]
        });
        assert_eq!(pick_model(&value).as_deref(), Some("qwen2.5-7b-instruct"));
    }

    #[test]
    fn auto_model_returns_none_for_embedding_only() {
        let value = json!({"data": [{"id": "bge-m3"}, {"id": "nomic-embed-text"}]});
        assert!(pick_model(&value).is_none());
    }

    #[test]
    fn maps_finish_reasons() {
        assert_eq!(map_finish_reason("tool_calls"), StopReason::ToolUse);
        assert_eq!(map_finish_reason("length"), StopReason::MaxTokens);
        assert_eq!(map_finish_reason("stop"), StopReason::EndTurn);
    }
}
