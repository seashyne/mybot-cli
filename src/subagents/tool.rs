use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::agent::{Agent, AgentEvent, AgentOptions};
use crate::mcp::McpManager;
use crate::permissions::{PermissionEngine, Prompter};
use crate::providers::{Provider, RetryPolicy};
use crate::skills::SkillSet;
use crate::tools::{ExecutionMode, Tool, ToolCtx, ToolOutput, ToolRegistry};

use super::{AgentDefinition, AgentSet};

const MAX_STEPS: usize = 24;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

const SUBAGENT_RULES: &str = r#"<subagent-rules>
You are a subagent working on one focused task for the parent agent.
- Work only on the task you were given; do not broaden the scope.
- You cannot ask the user questions. If you are blocked, stop and explain why.
- Do not spawn other agents.
- Finish with a concise report: what you did, what you found, and what the parent must know.
</subagent-rules>"#;

/// Everything a sub-agent needs that the parent agent already built.
pub struct AgentRuntime {
    pub provider: Arc<dyn Provider>,
    pub permissions: Arc<PermissionEngine>,
    pub prompter: Arc<dyn Prompter>,
    pub base_system: Vec<String>,
    pub cwd: PathBuf,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub thinking_budget: Option<u32>,
    pub retry: RetryPolicy,
    pub skills: Arc<SkillSet>,
    pub mcp: Option<Arc<McpManager>>,
    pub agents: Arc<AgentSet>,
}

pub struct AgentTool {
    runtime: Arc<AgentRuntime>,
}

impl AgentTool {
    pub fn new(runtime: Arc<AgentRuntime>) -> Self {
        Self { runtime }
    }

    fn registry(&self, definition: Option<&AgentDefinition>) -> ToolRegistry {
        let mut registry = ToolRegistry::with_skills(self.runtime.skills.clone());
        if let Some(mcp) = &self.runtime.mcp {
            for tool in mcp.tools() {
                registry.register(tool);
            }
        }
        if let Some(definition) = definition
            && let Some(allowed) = &definition.tools {
                registry.retain(allowed);
            }
        registry
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Delegate a focused, self-contained task to a sub-agent and return its report. Use it for parallel exploration or a narrow review; the sub-agent has its own context and cannot ask the user questions."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "Short label shown in the transcript (3-6 words)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Complete task for the sub-agent, including what to inspect and what to report"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "Optional agent type from the <subagents> list; omit for a general agent"
                }
            },
            "required": ["description", "prompt"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let prompt = args
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if prompt.is_empty() {
            return ToolOutput::error("agent requires a non-empty prompt");
        }

        let kind = args
            .get("subagent_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let definition = match kind {
            None => None,
            Some(name) => match self.runtime.agents.get(name) {
                Some(definition) => Some(definition.clone()),
                None => {
                    return ToolOutput::error(format!(
                        "unknown subagent_type '{name}' (available: {})",
                        self.runtime.agents.names().join(", ")
                    ));
                }
            },
        };

        let mut system = self.runtime.base_system.clone();
        if let Some(definition) = &definition {
            system.push(format!(
                "<subagent name=\"{}\">\n{}\n</subagent>",
                definition.name, definition.body
            ));
        }
        system.push(SUBAGENT_RULES.to_string());

        let tools = Arc::new(self.registry(definition.as_ref()));
        let mut agent = Agent::new(AgentOptions {
            provider: self.runtime.provider.clone(),
            tools,
            permissions: self.runtime.permissions.clone(),
            prompter: self.runtime.prompter.clone(),
            system,
            cwd: self.runtime.cwd.clone(),
            max_tokens: self.runtime.max_tokens,
            temperature: self.runtime.temperature,
            thinking_budget: self.runtime.thinking_budget,
            max_steps: Some(MAX_STEPS),
            session: None,
            messages: Vec::new(),
            retry: self.runtime.retry,
        });

        let (tx, rx) = mpsc::channel(256);
        let collector = tokio::spawn(collect(rx));
        let result = agent.run_turn(prompt, &tx, &ctx.cancel).await;
        drop(tx);
        let (text, trace) = collector.await.unwrap_or_default();

        let name = definition
            .as_ref()
            .map(|definition| definition.name.as_str())
            .unwrap_or("general");

        match result {
            Ok(_) => {
                let mut content = format!("<subagent name=\"{name}\" status=\"ok\">\n");
                if text.trim().is_empty() {
                    content.push_str("(the subagent returned no text)\n");
                } else {
                    content.push_str(text.trim());
                    content.push('\n');
                }
                if !trace.is_empty() {
                    content.push_str("\n<tools>\n");
                    for item in trace.iter().take(20) {
                        content.push_str(&format!("- {item}\n"));
                    }
                    content.push_str("</tools>\n");
                }
                content.push_str("</subagent>");
                ToolOutput::ok(cap(content))
            }
            Err(error) => {
                let mut content = format!("subagent '{name}' failed: {error}");
                if !text.trim().is_empty() {
                    content.push_str("\n\npartial report:\n");
                    content.push_str(text.trim());
                }
                if !trace.is_empty() {
                    content.push_str("\n\ntool trace:\n");
                    for item in trace.iter().take(20) {
                        content.push_str(&format!("- {item}\n"));
                    }
                }
                ToolOutput::error(cap(content))
            }
        }
    }

    fn summary(&self, args: &Value) -> String {
        let description = args
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("subagent");
        match args.get("subagent_type").and_then(Value::as_str) {
            Some(kind) if !kind.trim().is_empty() => format!("agent {kind} · {description}"),
            _ => format!("agent · {description}"),
        }
    }
}

async fn collect(mut rx: mpsc::Receiver<AgentEvent>) -> (String, Vec<String>) {
    let mut current = String::new();
    let mut last_turn = String::new();
    let mut trace = Vec::new();

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::TurnStart { .. } => current.clear(),
            AgentEvent::TextDelta(text) => current.push_str(&text),
            AgentEvent::TurnEnd { .. } => {
                if !current.trim().is_empty() {
                    last_turn = current.clone();
                }
                current.clear();
            }
            AgentEvent::ToolCall { name, summary, .. } => {
                trace.push(format!("{name}: {summary}"));
            }
            AgentEvent::Error(error) => trace.push(format!("error: {error}")),
            _ => {}
        }
    }

    (last_turn, trace)
}

fn cap(mut text: String) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text;
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[subagent output truncated]");
    text
}
