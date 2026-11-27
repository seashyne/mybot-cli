use serde_json::json;

use super::stream::{map_stop_reason, normalize_event};
use super::{build_body, messages_to_wire};
use crate::protocol::{ImageAttachment, Message, StopReason};
use crate::providers::sse::SseEvent;
use crate::providers::{CompletionRequest, ProviderEvent, ToolSpec};

fn req() -> CompletionRequest {
    CompletionRequest {
        model: "claude-test".into(),
        system: vec!["be helpful".into()],
        messages: vec![Message::user("hi")],
        tools: vec![ToolSpec {
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
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["tools"][0]["name"], "read");
    assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["messages"][0]["content"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["stream"], true);
    assert!(body["temperature"].as_f64().unwrap() > 0.19);
}

#[test]
fn usage_includes_prompt_cache_tokens() {
    let event = SseEvent {
        event: Some("message_start".into()),
        data: json!({
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 100,
                    "cache_creation_input_tokens": 500,
                    "cache_read_input_tokens": 2000,
                    "output_tokens": 0
                }
            }
        })
        .to_string(),
    };
    let events = normalize_event(&event);
    assert!(matches!(
        &events[0],
        Ok(ProviderEvent::Usage { input: 2600, output: 0 })
    ));
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
fn user_images_become_anthropic_base64_blocks() {
    let msgs = vec![Message::user_with_images(
        "look at this",
        vec![ImageAttachment {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            data_base64: "AQID".into(),
        }],
    )];
    let wire = messages_to_wire(&msgs);
    assert_eq!(wire[0]["content"][0]["type"], "image");
    assert_eq!(wire[0]["content"][0]["source"]["media_type"], "image/png");
    assert_eq!(wire[0]["content"][0]["source"]["data"], "AQID");
    assert_eq!(wire[0]["content"][1]["type"], "text");
    assert_eq!(wire[0]["content"][1]["text"], "look at this");
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
