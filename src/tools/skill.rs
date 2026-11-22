use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::skills::SkillSet;

use super::{ExecutionMode, Tool, ToolCtx, ToolOutput};

/// Loads the instructions of a named skill into the conversation.
pub struct SkillTool {
    skills: Arc<SkillSet>,
}

impl SkillTool {
    pub fn new(skills: Arc<SkillSet>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &'static str {
        "skill"
    }

    fn description(&self) -> &'static str {
        "Load the full instructions of a skill by name. Call this before following a skill's steps."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name as listed in the system prompt"
                }
            },
            "required": ["name"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();

        if name.is_empty() {
            return ToolOutput::error("missing skill name");
        }

        match self.skills.get(name) {
            Some(skill) => {
                let mut content = format!("# skill: {}\n\n", skill.name);
                content.push_str(&skill.body);
                if !skill.body.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(&format!("\n(source: {})", skill.path.display()));
                ToolOutput::ok(content)
            }
            None => {
                let available = self.skills.names().join(", ");
                ToolOutput::error(format!(
                    "unknown skill '{name}' (available: {available})"
                ))
            }
        }
    }

    fn summary(&self, args: &Value) -> String {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!("skill {name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::SkillSource;
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    fn ctx() -> ToolCtx {
        ToolCtx {
            cwd: std::env::temp_dir(),
            cancel: CancellationToken::new(),
            todos: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    fn set() -> Arc<SkillSet> {
        Arc::new(SkillSet {
            skills: vec![crate::skills::Skill {
                name: "review".into(),
                description: "review code".into(),
                body: "# Review\nRead the diff.".into(),
                path: PathBuf::from("/tmp/review/SKILL.md"),
                source: SkillSource::User,
            }],
        })
    }

    #[tokio::test]
    async fn returns_the_skill_body() {
        let tool = SkillTool::new(set());
        let output = tool.execute(&json!({"name": "review"}), &ctx()).await;
        assert!(!output.is_error);
        assert!(output.content.contains("# skill: review"));
        assert!(output.content.contains("Read the diff."));
    }

    #[tokio::test]
    async fn unknown_skill_lists_the_available_ones() {
        let tool = SkillTool::new(set());
        let output = tool.execute(&json!({"name": "nope"}), &ctx()).await;
        assert!(output.is_error);
        assert!(output.content.contains("review"));
    }
}
