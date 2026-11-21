use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::sse::{SseDecoder, SseEvent};
use super::{
    CompletionRequest, EventStream, Provider, ProviderError, ProviderEvent, join_url,
};
use crate::config::Resolved;
use crate::protocol::{Block, Message, StopReason};

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
        cancel: CancellationToken,
    ) -> Result<EventStream, ProviderError> {
        let url = messages_url(&self.resolved.base_url);
        let body = build_body(&req);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        if !self.resolved.api_key.is_empty() {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(&self.resolved.api_key)
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
                        for event in decoder.push(&bytes) {
                            for normalized in normalize_event(&event) {
                                if matches!(&normalized, Ok(ProviderEvent::MessageStop { .. })) {
                                    stop_sent = true;
                                }
                                yield normalized;
                            }
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

pub fn map_stop_reason(reason: &str) -> StopReason {
    match reason {
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "end_turn" | "stop_sequence" | "pause_turn" => StopReason::EndTurn,
        "refusal" => StopReason::Error,
        _ => StopReason::EndTurn,
    }
}

pub fn normalize_event(event: &SseEvent) -> Vec<Result<ProviderEvent, ProviderError>> {
    let mut out = Vec::new();
    let name = event.event.as_deref().unwrap_or_default();
    let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
        if event.data.trim().is_empty() || event.data.trim() == "[DONE]" {
            return out;
        }
        out.push(Err(ProviderError::protocol(format!(
            "invalid JSON in SSE event '{name}': {}",
            truncate(&event.data)
        ))));
        return out;
    };

    match name {
        "message_start" => {
            if let Some(usage) = value.pointer("/message/usage") {
                out.push(Ok(ProviderEvent::Usage {
                    input: usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
                    output: usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                }));
            }
        }
        "content_block_start" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let block = value.get("content_block").cloned().unwrap_or(Value::Null);
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                out.push(Ok(ProviderEvent::ToolCallStart {
                    index,
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                }));
            }
        }
        "content_block_delta" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let Some(delta) = value.get("delta") else {
                return out;
            };
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => {
                    if let Some(text) = delta.get("text").and_then(Value::as_str) {
                        out.push(Ok(ProviderEvent::TextDelta(text.to_string())));
                    }
                }
                Some("thinking_delta") => {
                    if let Some(text) = delta.get("thinking").and_then(Value::as_str) {
                        out.push(Ok(ProviderEvent::ThinkingDelta(text.to_string())));
                    }
                }
                Some("signature_delta") => {
                    if let Some(signature) = delta.get("signature").and_then(Value::as_str) {
                        out.push(Ok(ProviderEvent::ThinkingSignature(signature.to_string())));
                    }
                }
                Some("input_json_delta") => {
                    if let Some(partial) = delta.get("partial_json").and_then(Value::as_str) {
                        out.push(Ok(ProviderEvent::ToolCallArgsDelta {
                            index,
                            json: partial.to_string(),
                        }));
                    }
                }
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(usage) = value.get("usage") {
                out.push(Ok(ProviderEvent::Usage {
                    input: usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
                    output: usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                }));
            }
            if let Some(reason) = value
                .pointer("/delta/stop_reason")
                .and_then(Value::as_str)
            {
                out.push(Ok(ProviderEvent::MessageStop {
                    stop_reason: map_stop_reason(reason),
                }));
            }
        }
        "error" => {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("provider stream error");
            out.push(Err(ProviderError::protocol(message.to_string())));
        }
        _ => {}
    }

    out
}

fn truncate(value: &str) -> String {
    if value.len() > 200 {
        format!("{}…", &value[..200])
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> CompletionRequest {
        CompletionRequest {
            model: "claude-test".into(),
            system: vec!["be helpful".into()],
            messages: vec![Message::user("hi")],
            tools: vec![super::super::ToolSpec {
                name: "read".into(),
                description: "read a file".into(),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }],
            max_tokens: 1024,
            temperature: Some(0.2),
            thinking_budget: None,
        }
    }

    #[test]
    fn body_places_system_and_tools() {
        let body = build_body(&req());
        assert_eq!(body["system"][0]["text"], "be helpful");
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["stream"], true);
        assert!(body["temperature"].as_f64().unwrap() > 0.19);
    }

    #[test]
    fn thinking_disables_temperature() {
        let mut request = req();
        request.thinking_budget = Some(2048);
        let body = build_body(&request);
        assert!(body.get("temperature").is_none());
        assert_eq!(body["thinking"]["budget_tokens"], 2048);
    }

    #[test]
    fn tool_results_coalesce_into_one_user_turn() {
        let messages = vec![Message::ToolResults {
            results: vec![
                crate::protocol::ToolResult {
                    tool_call_id: "a".into(),
                    tool_name: "read".into(),
                    content: "one".into(),
                    is_error: false,
                },
                crate::protocol::ToolResult {
                    tool_call_id: "b".into(),
                    tool_name: "read".into(),
                    content: "two".into(),
                    is_error: false,
                },
            ],
        }];
        let wire = messages_to_wire(&messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn normalizes_text_and_tool_deltas() {
        let start = SseEvent {
            event: Some("content_block_start".into()),
            data: json!({
                "index": 1,
                "content_block": {"type": "tool_use", "id": "call_1", "name": "read"}
            })
            .to_string(),
        };
        let events = normalize_event(&start);
        assert!(matches!(
            &events[0],
            Ok(ProviderEvent::ToolCallStart { index: 1, .. })
        ));

        let delta = SseEvent {
            event: Some("content_block_delta".into()),
            data: json!({
                "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": "{\"path\""}
            })
            .to_string(),
        };
        let events = normalize_event(&delta);
        assert!(matches!(
            &events[0],
            Ok(ProviderEvent::ToolCallArgsDelta { json, .. }) if json == "{\"path\""
        ));
    }

    #[test]
    fn maps_stop_reasons() {
        assert_eq!(map_stop_reason("tool_use"), StopReason::ToolUse);
        assert_eq!(map_stop_reason("max_tokens"), StopReason::MaxTokens);
        assert_eq!(map_stop_reason("end_turn"), StopReason::EndTurn);
    }
}
