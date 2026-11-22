use crate::protocol::{Block, Message};
use crate::tools::truncate::head_tail;

pub const SYSTEM_PROMPT: &str = r#"You are compacting a coding conversation so the assistant can keep working in a fresh context window.

Write a thorough handoff summary of the conversation so far. Preserve, in this order:
- The user's goals, constraints, and explicit instructions.
- Decisions made, and the reasons behind them.
- Files inspected or changed, with paths, and what changed in each.
- Commands run and the important results or errors.
- The current state: what is finished, what is in progress, and what remains.
- Open questions, blockers, or risks.

Rules:
- Do not call tools.
- Do not ask questions or address the user directly.
- Do not refer to "the conversation above" or to being a summary.
- Output only the summary, as compact markdown. Keep it under 1500 words."#;

pub const INSTRUCTION: &str = "Write the handoff summary now.";

pub const SUMMARY_PREFIX: &str = "[compacted conversation]";

/// Cap for one rendered message (bytes). Tool output is the usual offender.
const MAX_MESSAGE_BYTES: usize = 8_000;

/// Cap for the whole flattened transcript (bytes, roughly 30k tokens).
const MAX_TRANSCRIPT_BYTES: usize = 120_000;

/// The synthetic user message that replaces the history after compaction.
pub fn summary_message(summary: &str) -> Message {
    Message::user(format!(
        "{SUMMARY_PREFIX}\n\nEarlier turns were compacted to save context. Handoff summary:\n\n{summary}"
    ))
}

pub fn is_summary_message(message: &Message) -> bool {
    matches!(message, Message::User { content, .. } if content.starts_with(SUMMARY_PREFIX))
}

/// One user message containing the flattened conversation and the instruction.
///
/// Flattening avoids provider-specific history rules (role alternation, dangling
/// tool calls, thinking blocks) and lets us bound the request size.
pub fn request_messages(messages: &[Message]) -> Vec<Message> {
    let transcript = build_transcript(messages);
    vec![Message::user(format!(
        "<conversation>\n{transcript}\n</conversation>\n\n{INSTRUCTION}"
    ))]
}

pub fn build_transcript(messages: &[Message]) -> String {
    if messages.is_empty() {
        return String::new();
    }

    let rendered: Vec<String> = messages.iter().map(render_message).collect();
    let total: usize = rendered.iter().map(|entry| entry.len() + 2).sum();
    if total <= MAX_TRANSCRIPT_BYTES {
        return rendered.join("\n\n");
    }

    // Keep the opening request (the original goal) and the most recent turns.
    let first = &rendered[0];
    let marker = "[earlier messages omitted]";
    let mut used = first.len() + marker.len() + 4;
    let mut selected: Vec<&str> = Vec::new();
    for entry in rendered.iter().skip(1).rev() {
        let len = entry.len() + 2;
        if used + len > MAX_TRANSCRIPT_BYTES {
            break;
        }
        selected.push(entry);
        used += len;
    }
    selected.reverse();

    let mut out = String::new();
    out.push_str(first);
    out.push_str("\n\n");
    out.push_str(marker);
    out.push_str("\n\n");
    out.push_str(&selected.join("\n\n"));
    out
}

fn render_message(message: &Message) -> String {
    match message {
        Message::User { content, images } => {
            let (text, _) = head_tail(content, MAX_MESSAGE_BYTES);
            if images.is_empty() {
                format!("[user]\n{text}")
            } else {
                format!("[user]\n[attached {} image(s)]\n{text}", images.len())
            }
        }
        Message::Assistant { content } => {
            let mut parts: Vec<String> = Vec::new();
            let mut text = String::new();

            for block in content {
                match block {
                    Block::Text { text: value } => text.push_str(value),
                    // Thinking is internal and may be rejected when replayed
                    // without a thinking budget, so leave it out.
                    Block::Thinking { .. } => {}
                    Block::ToolCall {
                        name, arguments, ..
                    } => {
                        if !text.trim().is_empty() {
                            parts.push(text.trim().to_string());
                            text.clear();
                        }
                        let args = arguments.to_string();
                        let (args, _) = head_tail(&args, MAX_MESSAGE_BYTES);
                        parts.push(format!("called {name} {args}"));
                    }
                }
            }
            if !text.trim().is_empty() {
                parts.push(text.trim().to_string());
            }

            let body = if parts.is_empty() {
                "(empty)".to_string()
            } else {
                parts.join("\n")
            };
            let (body, _) = head_tail(&body, MAX_MESSAGE_BYTES);
            format!("[assistant]\n{body}")
        }
        Message::ToolResults { results } => {
            let mut parts = Vec::new();
            for result in results {
                let (content, _) = head_tail(&result.content, MAX_MESSAGE_BYTES);
                let status = if result.is_error { " (error)" } else { "" };
                parts.push(format!("[tool {}{status}]\n{content}", result.tool_name));
            }
            parts.join("\n\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_message_is_marked() {
        let message = summary_message("did things");
        assert!(is_summary_message(&message));
        assert!(message.text().contains("did things"));
        assert!(message.text().starts_with(SUMMARY_PREFIX));
        assert!(!is_summary_message(&Message::user("ordinary")));
    }

    #[test]
    fn transcript_includes_roles_tools_and_skips_thinking() {
        let messages = vec![
            Message::user("fix the bug"),
            Message::Assistant {
                content: vec![
                    Block::Thinking {
                        text: "internal reasoning".into(),
                        signature: None,
                    },
                    Block::Text {
                        text: "looking".into(),
                    },
                    Block::ToolCall {
                        id: "c1".into(),
                        name: "read".into(),
                        arguments: serde_json::json!({"path": "a.rs"}),
                    },
                ],
            },
            Message::ToolResults {
                results: vec![crate::protocol::ToolResult {
                    tool_call_id: "c1".into(),
                    tool_name: "read".into(),
                    content: "fn main() {}".into(),
                    is_error: false,
                }],
            },
        ];

        let transcript = build_transcript(&messages);
        assert!(transcript.contains("[user]\nfix the bug"));
        assert!(transcript.contains("[assistant]"));
        assert!(transcript.contains("looking"));
        assert!(transcript.contains("called read"));
        assert!(transcript.contains("[tool read]"));
        assert!(transcript.contains("fn main() {}"));
        assert!(!transcript.contains("internal reasoning"));
    }

    #[test]
    fn long_messages_are_truncated() {
        let messages = vec![Message::user("x".repeat(20_000))];
        let transcript = build_transcript(&messages);
        assert!(transcript.len() < 20_000);
        assert!(transcript.contains("bytes omitted"));
    }

    #[test]
    fn oversized_transcript_keeps_first_and_recent_turns() {
        let mut messages = vec![Message::user("FIRST GOAL")];
        for index in 0..40 {
            messages.push(Message::user(format!("turn {index} {}", "y".repeat(7_000))));
        }
        messages.push(Message::user("LAST TURN"));

        let transcript = build_transcript(&messages);
        assert!(transcript.len() <= MAX_TRANSCRIPT_BYTES + 100);
        assert!(transcript.starts_with("[user]\nFIRST GOAL"));
        assert!(transcript.contains("[earlier messages omitted]"));
        assert!(transcript.contains("LAST TURN"));
    }

    #[test]
    fn request_wraps_transcript_and_instruction() {
        let messages = vec![Message::user("hello")];
        let request = request_messages(&messages);
        assert_eq!(request.len(), 1);
        let text = request[0].text();
        assert!(text.starts_with("<conversation>"));
        assert!(text.contains("[user]\nhello"));
        assert!(text.ends_with(INSTRUCTION));
    }
}
