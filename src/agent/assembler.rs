use std::collections::BTreeMap;

use serde_json::Value;

use crate::protocol::Block;

/// One tool call reconstructed from streamed deltas.
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub id: String,
    pub name: String,
    pub arguments: Result<Value, String>,
}

#[derive(Debug, Default)]
struct PartialTool {
    id: String,
    name: String,
    json: String,
}

#[derive(Debug, Default)]
pub struct Assembler {
    text: String,
    thinking: String,
    signature: Option<String>,
    tools: BTreeMap<usize, PartialTool>,
}

pub struct Assembled {
    pub blocks: Vec<Block>,
    pub invocations: Vec<ToolInvocation>,
    pub text: String,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_text(&mut self, delta: &str) {
        self.text.push_str(delta);
    }

    pub fn on_thinking(&mut self, delta: &str) {
        self.thinking.push_str(delta);
    }

    pub fn on_signature(&mut self, delta: &str) {
        match &mut self.signature {
            Some(signature) => signature.push_str(delta),
            None => self.signature = Some(delta.to_string()),
        }
    }

    pub fn on_tool_start(&mut self, index: usize, id: &str, name: &str) {
        let entry = self.tools.entry(index).or_default();
        if !id.is_empty() {
            entry.id = id.to_string();
        }
        if !name.is_empty() {
            entry.name = name.to_string();
        }
    }

    pub fn on_tool_args(&mut self, index: usize, json: &str) {
        let entry = self.tools.entry(index).or_default();
        entry.json.push_str(json);
    }

    pub fn finish(mut self) -> Assembled {
        if self.thinking.is_empty() && self.text.contains("<think>") {
            let (cleaned, extracted) = extract_tagged_blocks(&self.text, "<think>", "</think>");
            if !extracted.is_empty() {
                self.thinking = extracted.join("\n").trim().to_string();
                self.text = cleaned;
            }
        }

        if self.tools.is_empty() && self.text.contains("<tool_call>") {
            let (cleaned, raw_calls) =
                extract_tagged_blocks(&self.text, "<tool_call>", "</tool_call>");
            for raw in raw_calls {
                if let Some((name, args_json)) = parse_xml_tool_call(&raw) {
                    let index = self.tools.len();
                    self.tools.insert(
                        index,
                        PartialTool {
                            id: format!("call_{index}"),
                            name,
                            json: args_json,
                        },
                    );
                }
            }
            if !self.tools.is_empty() {
                self.text = cleaned.trim().to_string();
            }
        }

        let mut blocks = Vec::new();
        if !self.text.is_empty() {
            blocks.push(Block::Text {
                text: self.text.clone(),
            });
        }
        if !self.thinking.is_empty() {
            blocks.push(Block::Thinking {
                text: self.thinking,
                signature: self.signature,
            });
        }

        let mut invocations = Vec::new();
        for (index, partial) in &self.tools {
            let name = if partial.name.is_empty() {
                "unknown".to_string()
            } else {
                partial.name.clone()
            };
            let id = if partial.id.is_empty() {
                format!("call_{index}")
            } else {
                partial.id.clone()
            };
            let trimmed = partial.json.trim();
            let parsed = if trimmed.is_empty() {
                Ok(Value::Object(Default::default()))
            } else {
                serde_json::from_str::<Value>(trimmed)
                    .map_err(|error| format!("invalid JSON arguments for {name}: {error}"))
            };
            let arguments_for_history = match &parsed {
                Ok(value) if value.is_object() => value.clone(),
                _ => Value::Object(Default::default()),
            };
            blocks.push(Block::ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments_for_history,
            });
            invocations.push(ToolInvocation {
                id,
                name,
                arguments: parsed,
            });
        }

        Assembled {
            blocks,
            invocations,
            text: self.text,
        }
    }
}

fn extract_tagged_blocks(text: &str, open: &str, close: &str) -> (String, Vec<String>) {
    let mut cleaned = String::with_capacity(text.len());
    let mut blocks = Vec::new();
    let mut rest = text;

    while let Some(start) = rest.find(open) {
        cleaned.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        if let Some(end) = after_open.find(close) {
            blocks.push(after_open[..end].trim().to_string());
            rest = &after_open[end + close.len()..];
        } else {
            // Unclosed tag: leave the remainder untouched.
            cleaned.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    cleaned.push_str(rest);
    (cleaned, blocks)
}

fn parse_xml_tool_call(raw: &str) -> Option<(String, String)> {
    let trimmed = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let name = ["name", "tool", "function"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))?
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let args = ["arguments", "parameters", "args", "input"]
        .into_iter()
        .find_map(|key| value.get(key))
        .map(|val| match val {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "{}".to_string());
    Some((name, args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_text_and_tool_calls() {
        let mut assembler = Assembler::new();
        assembler.on_text("Let me ");
        assembler.on_text("check.");
        assembler.on_tool_start(0, "call_1", "read");
        assembler.on_tool_args(0, "{\"path\":");
        assembler.on_tool_args(0, "\"a.txt\"}");

        let assembled = assembler.finish();
        assert_eq!(assembled.text, "Let me check.");
        assert_eq!(assembled.invocations.len(), 1);
        assert_eq!(assembled.invocations[0].name, "read");
        let args = assembled.invocations[0].arguments.as_ref().unwrap();
        assert_eq!(args["path"], "a.txt");
    }

    #[test]
    fn invalid_json_becomes_error() {
        let mut assembler = Assembler::new();
        assembler.on_tool_start(0, "c1", "edit");
        assembler.on_tool_args(0, "{oops");
        let assembled = assembler.finish();
        assert!(assembled.invocations[0].arguments.is_err());
        // History still carries a valid (empty) object so the wire format stays valid.
        match &assembled.blocks[0] {
            Block::ToolCall { arguments, .. } => assert!(arguments.is_object()),
            _ => panic!("expected tool call"),
        }
    }

    #[test]
    fn missing_arguments_default_to_empty_object() {
        let mut assembler = Assembler::new();
        assembler.on_tool_start(0, "c1", "todowrite");
        let assembled = assembler.finish();
        let args = assembled.invocations[0].arguments.as_ref().unwrap();
        assert!(args.is_object());
    }

    #[test]
    fn multiple_tools_keep_index_order() {
        let mut assembler = Assembler::new();
        assembler.on_tool_start(1, "c2", "glob");
        assembler.on_tool_start(0, "c1", "read");
        assembler.on_tool_args(0, "{}");
        assembler.on_tool_args(1, "{}");
        let assembled = assembler.finish();
        assert_eq!(assembled.invocations[0].name, "read");
        assert_eq!(assembled.invocations[1].name, "glob");
    }

    #[test]
    fn extracts_xml_tool_call_and_think_blocks_from_local_model_text() {
        let mut assembler = Assembler::new();
        assembler.on_text(
            "<think>Need to run cargo test</think>Running tests now.\n<tool_call>{\"name\": \"bash\", \"arguments\": {\"command\": \"cargo test\"}}</tool_call>",
        );
        let assembled = assembler.finish();
        assert_eq!(assembled.text, "Running tests now.");
        assert_eq!(assembled.invocations.len(), 1);
        assert_eq!(assembled.invocations[0].name, "bash");
        let args = assembled.invocations[0].arguments.as_ref().unwrap();
        assert_eq!(args["command"], "cargo test");
    }
}
