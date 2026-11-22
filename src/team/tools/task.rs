use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::{ExecutionMode, Tool, ToolCtx, ToolOutput};

use super::super::Team;
use super::cap;

pub(super) struct TaskCreate {
    team: Arc<Team>,
}

impl TaskCreate {
    pub(super) fn new(team: Arc<Team>) -> Self {
        Self { team }
    }
}

#[async_trait]
impl Tool for TaskCreate {
    fn name(&self) -> &str {
        "task_create"
    }

    fn description(&self) -> &str {
        "Add a task to the shared team task board. Use one task per independent piece of work, then assign it with task_update and spawn a teammate for it."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subject": {"type": "string", "description": "Short imperative title"},
                "description": {"type": "string", "description": "What must be done and what counts as done"}
            },
            "required": ["subject"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let subject = args
            .get("subject")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if subject.is_empty() {
            return ToolOutput::error("task_create requires a non-empty subject");
        }
        let description = args
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let task = self.team.create_task(subject, description).await;
        ToolOutput::ok(format!("created task [{}] {}", task.id, task.subject))
    }

    fn summary(&self, args: &Value) -> String {
        args.get("subject")
            .and_then(Value::as_str)
            .unwrap_or("task")
            .to_string()
    }
}

pub(super) struct TaskList {
    team: Arc<Team>,
}

impl TaskList {
    pub(super) fn new(team: Arc<Team>) -> Self {
        Self { team }
    }
}

#[async_trait]
impl Tool for TaskList {
    fn name(&self) -> &str {
        "task_list"
    }

    fn description(&self) -> &str {
        "Show the shared team task board: ids, status, owners, and descriptions."
    }

    fn schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn execute(&self, _args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let tasks = self.team.tasks().await;
        if tasks.is_empty() {
            return ToolOutput::ok("the task board is empty");
        }
        let mut out = String::new();
        for task in tasks {
            let owner = task
                .owner
                .as_deref()
                .map(|owner| format!(" @{owner}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "[{}] {:<12} {}{owner}\n",
                task.id, task.status, task.subject
            ));
            if !task.description.is_empty() {
                for line in task.description.lines() {
                    out.push_str(&format!("    {line}\n"));
                }
            }
        }
        ToolOutput::ok(cap(out))
    }
}

pub(super) struct TaskUpdate {
    team: Arc<Team>,
    identity: String,
}

impl TaskUpdate {
    pub(super) fn new(team: Arc<Team>, identity: &str) -> Self {
        Self {
            team,
            identity: identity.to_string(),
        }
    }
}

#[async_trait]
impl Tool for TaskUpdate {
    fn name(&self) -> &str {
        "task_update"
    }

    fn description(&self) -> &str {
        "Update a task on the shared board. Set status to pending, in_progress, or completed; set owner to claim it."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "Task id such as t1"},
                "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]},
                "owner": {"type": "string", "description": "Teammate name that owns the task"},
                "subject": {"type": "string", "description": "Replacement title"}
            },
            "required": ["id"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if id.is_empty() {
            return ToolOutput::error("task_update requires a task id");
        }
        let status = args.get("status").and_then(Value::as_str);
        let subject = args.get("subject").and_then(Value::as_str);
        let owner = match args.get("owner").and_then(Value::as_str) {
            Some(owner) => Some(owner),
            None if self.identity != "lead" => Some(self.identity.as_str()),
            None => None,
        };

        match self.team.update_task(id, status, owner, subject).await {
            Ok(task) => {
                let owner = task
                    .owner
                    .map(|owner| format!(" @{owner}"))
                    .unwrap_or_default();
                ToolOutput::ok(format!(
                    "updated [{}] {} {}{owner}",
                    task.id, task.status, task.subject
                ))
            }
            Err(error) => ToolOutput::error(error),
        }
    }

    fn summary(&self, args: &Value) -> String {
        let id = args.get("id").and_then(Value::as_str).unwrap_or("?");
        match args.get("status").and_then(Value::as_str) {
            Some(status) => format!("{id} → {status}"),
            None => id.to_string(),
        }
    }
}
