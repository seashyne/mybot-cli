use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub name: String,
    pub media_type: String,
    pub data_base64: String,
}

/// Canonical, provider-independent conversation message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    User {
        content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageAttachment>,
    },
    Assistant {
        content: Vec<Block>,
    },
    /// All tool results produced by one assistant turn, coalesced into a single
    /// logical message so each wire format can render it correctly.
    ToolResults {
        results: Vec<ToolResult>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    Thinking {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Error,
    Aborted,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message::User {
            content: text.into(),
            images: Vec::new(),
        }
    }

    pub fn user_with_images(text: impl Into<String>, images: Vec<ImageAttachment>) -> Self {
        Message::User {
            content: text.into(),
            images,
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Message::Assistant {
            content: vec![Block::Text { text: text.into() }],
        }
    }

    pub fn text(&self) -> String {
        match self {
            Message::User { content, .. } => content.clone(),
            Message::Assistant { content } => content
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
            Message::ToolResults { results } => results
                .iter()
                .map(|r| format!("[{}] {}", r.tool_name, r.content))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

impl Block {
    pub fn text(&self) -> Option<&str> {
        match self {
            Block::Text { text } => Some(text),
            _ => None,
        }
    }

    pub fn tool_call(&self) -> Option<(&str, &str, &serde_json::Value)> {
        match self {
            Block::ToolCall {
                id,
                name,
                arguments,
            } => Some((id, name, arguments)),
            _ => None,
        }
    }
}

/// Very rough token estimate (chars / 4). Good enough for status display.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    let chars: usize = messages
        .iter()
        .map(|m| match m {
            Message::User { content, images } => content.len() + images.len() * 1024,
            Message::Assistant { content } => content
                .iter()
                .map(|b| match b {
                    Block::Text { text } => text.len(),
                    Block::Thinking { text, .. } => text.len(),
                    Block::ToolCall { name, arguments, .. } => name.len() + arguments.to_string().len(),
                })
                .sum(),
            Message::ToolResults { results } => results.iter().map(|r| r.content.len()).sum(),
        })
        .sum();
    chars / 4
}

/// Standard RFC 4648 base64 encoder without external dependencies.
pub fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[(triple >> 18) & 0x3f] as char);
        out.push(TABLE[(triple >> 12) & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(triple >> 6) & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[triple & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_json_round_trip() {
        let msg = Message::Assistant {
            content: vec![
                Block::Text {
                    text: "hi".into(),
                },
                Block::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({"path": "a.txt"}),
                },
            ],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn estimate_counts_text() {
        let msgs = vec![Message::user("abcdefgh")];
        assert_eq!(estimate_tokens(&msgs), 2);
    }
}
