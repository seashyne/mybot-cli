use futures_util::StreamExt;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::AnthropicProvider;
use crate::providers::sse::{SseDecoder, SseEvent};
use crate::providers::{CompletionRequest, EventStream, ProviderError, ProviderEvent};
use crate::protocol::StopReason;

pub(super) async fn open(
    provider: &AnthropicProvider,
    req: CompletionRequest,
    cancel: CancellationToken,
) -> Result<EventStream, ProviderError> {
    let url = super::messages_url(&provider.resolved.base_url);
    let body = super::build_body(&req);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    if !provider.resolved.api_key.is_empty() {
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&provider.resolved.api_key)
                .map_err(|_| ProviderError::config("API key contains invalid header characters"))?,
        );
    }
    for (key, value) in &provider.resolved.headers {
        let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| ProviderError::config(format!("invalid header name '{key}'")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| ProviderError::config(format!("invalid header value for '{key}'")))?;
        headers.insert(name, value);
    }

    let response = provider
        .client
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::network(format!("request to {url} failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let retry_after = crate::providers::retry_after_header(&response);
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::from_status(status.as_u16(), &text)
            .with_retry_after(retry_after));
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

pub(super) fn map_stop_reason(reason: &str) -> StopReason {
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
