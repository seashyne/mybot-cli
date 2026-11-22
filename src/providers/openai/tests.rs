use serde_json::json;

use super::stream::{map_finish_reason, normalize_event};
use super::{build_body, completions_url, messages_to_wire, models_url, pick_model};
use crate::protocol::{Message, StopReason};
use crate::providers::sse::SseEvent;
use crate::providers::{CompletionRequest, ProviderEvent, ToolSpec};

fn req() -> CompletionRequest {
    CompletionRequest {
        model: "gpt-4o-mini".into(),
        system: vec!["sys".into()],
        messages: vec![Message::user("hi")],
        tools: vec![ToolSpec {
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

#[test]
fn urls_follow_the_shape_of_the_base() {
    assert_eq!(
        models_url("https://api.openai.com/v1"),
        "https://api.openai.com/v1/models"
    );
    assert_eq!(
        models_url("https://generativelanguage.googleapis.com/v1beta/openai/"),
        "https://generativelanguage.googleapis.com/v1beta/openai/models"
    );
    assert_eq!(
        models_url("http://127.0.0.1:1234"),
        "http://127.0.0.1:1234/v1/models"
    );
    assert_eq!(
        completions_url("https://generativelanguage.googleapis.com/v1beta/openai/"),
        "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
    );
    assert_eq!(
        completions_url("https://api.z.ai/api/paas/v4"),
        "https://api.z.ai/api/paas/v4/chat/completions"
    );
}
