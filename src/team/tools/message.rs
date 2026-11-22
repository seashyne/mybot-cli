use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::{ExecutionMode, Tool, ToolCtx, ToolOutput};

use super::super::Team;
use super::cap;

const MAX_WAIT_SECS: u64 = 300;

pub(super) struct SendMessage {
    team: Arc<Team>,
    identity: String,
}

impl SendMessage {
    pub(super) fn new(team: Arc<Team>, identity: &str) -> Self {
        Self {
            team,
            identity: identity.to_string(),
        }
    }
}

#[async_trait]
impl Tool for SendMessage {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to the lead (to: \"lead\") or to another teammate by name. Messages are delivered to the recipient's inbox and read with check_messages."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {"type": "string", "description": "Recipient: lead, or a teammate name"},
                "content": {"type": "string", "description": "Message body"}
            },
            "required": ["to", "content"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let to = args.get("to").and_then(Value::as_str).unwrap_or_default();
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match self.team.send_message(&self.identity, to, content).await {
            Ok(()) => ToolOutput::ok(format!("message sent to {to}")),
            Err(error) => ToolOutput::error(error),
        }
    }

    fn summary(&self, args: &Value) -> String {
        let to = args.get("to").and_then(Value::as_str).unwrap_or("?");
        format!("→ {to}")
    }
}

pub(super) struct CheckMessages {
    team: Arc<Team>,
    identity: String,
}

impl CheckMessages {
    pub(super) fn new(team: Arc<Team>, identity: &str) -> Self {
        Self {
            team,
            identity: identity.to_string(),
        }
    }
}

#[async_trait]
impl Tool for CheckMessages {
    fn name(&self) -> &str {
        "check_messages"
    }

    fn description(&self) -> &str {
        "Read and clear your inbox. Set wait_secs to first wait until all teammates are idle, which is useful for collecting reports."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "wait_secs": {
                    "type": "integer",
                    "description": "Wait up to this many seconds for teammates to finish (0-300, default 0)"
                }
            }
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let wait = args
            .get("wait_secs")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(MAX_WAIT_SECS);
        if wait > 0 {
            self.team.wait_idle(Duration::from_secs(wait)).await;
        }
        let messages = self.team.drain_inbox(&self.identity).await;
        if messages.is_empty() {
            return ToolOutput::ok("(no messages)");
        }
        let mut out = String::new();
        for message in messages {
            out.push_str(&format!("from {}:\n{}\n\n", message.from, message.content));
        }
        ToolOutput::ok(cap(out))
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("wait_secs").and_then(Value::as_u64).unwrap_or(0) {
            0 => "inbox".to_string(),
            secs => format!("inbox (wait {secs}s)"),
        }
    }
}
