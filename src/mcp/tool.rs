use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::mcp::client::{Client, McpToolDef};
use crate::tools::{ExecutionMode, Tool, ToolCtx, ToolOutput};

/// Wraps one MCP tool so the agent can call it like a built-in tool.
pub struct McpTool {
    wire_name: String,
    server: String,
    tool: String,
    description: String,
    schema: Value,
    client: Arc<Client>,
}

impl McpTool {
    pub fn new(server: &str, definition: McpToolDef, client: Arc<Client>) -> Self {
        let wire_name = wire_name(server, &definition.name);
        Self {
            wire_name,
            server: server.to_string(),
            tool: definition.name,
            description: definition.description,
            schema: definition.input_schema,
            client,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.wire_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        match self.client.call_tool(&self.tool, args.clone()).await {
            Ok(result) if result.is_error => ToolOutput::error(result.content),
            Ok(result) => ToolOutput::ok(result.content),
            Err(error) => ToolOutput::error(format!(
                "mcp {} / {}: {error}",
                self.server, self.tool
            )),
        }
    }

    fn summary(&self, _args: &Value) -> String {
        format!("mcp {}/{}", self.server, self.tool)
    }
}

/// Provider tool names must be short and safe: `mcp__<server>__<tool>`.
pub fn wire_name(server: &str, tool: &str) -> String {
    let raw = format!("mcp__{}__{}", sanitize(server), sanitize(tool));
    if raw.len() <= 64 {
        return raw;
    }
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    let suffix = format!("_{:08x}", hasher.finish() as u32);
    let keep = 64 - suffix.len();
    format!("{}{suffix}", &raw[..keep])
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_prefixed_and_sanitized() {
        assert_eq!(wire_name("fs", "read_file"), "mcp__fs__read_file");
        assert_eq!(wire_name("my server", "read.file"), "mcp__my_server__read_file");
    }

    #[test]
    fn long_names_are_capped_and_unique() {
        let tool = "x".repeat(80);
        let name = wire_name("server", &tool);
        assert_eq!(name.len(), 64);
        let other = wire_name("server", &"y".repeat(80));
        assert_ne!(name, other);
    }
}
