use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::{ExecutionMode, Tool, ToolCtx, ToolOutput};

use super::super::runner::TeammateRequest;
use super::super::Team;

pub(super) struct SpawnTeammate {
    team: Arc<Team>,
}

impl SpawnTeammate {
    pub(super) fn new(team: Arc<Team>) -> Self {
        Self { team }
    }
}

#[async_trait]
impl Tool for SpawnTeammate {
    fn name(&self) -> &str {
        "spawn_teammate"
    }

    fn description(&self) -> &str {
        "Start a background teammate with its own context and tools. It works on the task you describe, updates the shared task board, and reports back to your inbox. Use check_messages to collect its report."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Short unique teammate name, e.g. parser or tester"},
                "role": {"type": "string", "description": "One-line role, e.g. backend engineer or reviewer"},
                "prompt": {"type": "string", "description": "Complete assignment: what to do, where to look, and what to report"},
                "tools": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional tool whitelist (team tools are always available)"
                }
            },
            "required": ["name", "prompt"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let role = args.get("role").and_then(Value::as_str).unwrap_or_default();
        let prompt = args
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let tools = args.get("tools").and_then(Value::as_array).map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        });

        let request = TeammateRequest {
            name: name.to_string(),
            role: role.to_string(),
            prompt: prompt.to_string(),
            tools,
        };
        match self.team.spawn(request).await {
            Ok(name) => ToolOutput::ok(format!(
                "teammate '{name}' started in the background; its report will arrive in your inbox (check_messages)"
            )),
            Err(error) => ToolOutput::error(error),
        }
    }

    fn summary(&self, args: &Value) -> String {
        let name = args.get("name").and_then(Value::as_str).unwrap_or("?");
        match args.get("role").and_then(Value::as_str) {
            Some(role) if !role.trim().is_empty() => format!("{name} · {role}"),
            _ => name.to_string(),
        }
    }
}
