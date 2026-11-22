use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::OpenAiProvider;
use crate::providers::sse::{SseDecoder, SseEvent};
use crate::providers::{CompletionRequest, EventStream, ProviderError, ProviderEvent};
use crate::protocol::StopReason;

pub(super) async fn open(
    provider: &OpenAiProvider,
    req: CompletionRequest,
    cancel: CancellationToken,
) -> Result<EventStream, ProviderError> {
    let url = super::completions_url(&provider.resolved.base_url);
    let mut req = req;
    req.model = provider.resolve_model(&req.model).await?;
    let body = super::build_body(&req);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if !provider.resolved.api_key.is_empty() {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", provider.resolved.api_key))
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

pub(super) fn map_finish_reason(reason: &str) -> StopReason {
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
