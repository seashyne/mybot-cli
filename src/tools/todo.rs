use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ExecutionMode, TodoItem, Tool, ToolCtx, ToolOutput};

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &'static str {
        "todowrite"
    }

    fn description(&self) -> &'static str {
        "Replace the session todo list. Use it for multi-step work and keep exactly one item in_progress at a time."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string", "description": "What needs to be done"},
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"]
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["high", "medium", "low"]
                            }
                        },
                        "required": ["content", "status", "priority"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(list) = args.get("todos").and_then(Value::as_array) else {
            return ToolOutput::error("missing required argument: todos (array)");
        };

        let mut todos = Vec::with_capacity(list.len());
        for (index, entry) in list.iter().enumerate() {
            let Some(content) = entry.get("content").and_then(Value::as_str) else {
                return ToolOutput::error(format!("todos[{index}] is missing content"));
            };
            let status = normalize_status(
                entry
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("pending"),
            );
            let priority = normalize_priority(
                entry
                    .get("priority")
                    .and_then(Value::as_str)
                    .unwrap_or("medium"),
            );
            todos.push(TodoItem {
                content: content.trim().to_string(),
                status,
                priority,
            });
        }

        *ctx.todos.lock().await = todos.clone();

        if todos.is_empty() {
            return ToolOutput::ok("Todo list cleared");
        }

        let mut out = String::new();
        for (index, todo) in todos.iter().enumerate() {
            let marker = match todo.status.as_str() {
                "completed" => "x",
                "in_progress" => "~",
                _ => " ",
            };
            out.push_str(&format!(
                "[{marker}] {}. {} ({})\n",
                index + 1,
                todo.content,
                todo.priority
            ));
        }
        ToolOutput::ok(out)
    }
}

fn normalize_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "completed" | "done" | "complete" => "completed".into(),
        "in_progress" | "doing" | "active" | "started" => "in_progress".into(),
        _ => "pending".into(),
    }
}

fn normalize_priority(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "high" | "urgent" | "p0" => "high".into(),
        "low" | "later" | "p3" => "low".into(),
        _ => "medium".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    fn ctx() -> ToolCtx {
        ToolCtx {
            cwd: std::env::temp_dir(),
            cancel: CancellationToken::new(),
            todos: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[tokio::test]
    async fn stores_todos() {
        let ctx = ctx();
        let out = TodoWriteTool
            .execute(
                &json!({"todos": [
                    {"content": "write code", "status": "in_progress", "priority": "high"},
                    {"content": "test", "status": "pending", "priority": "medium"}
                ]}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("[~] 1. write code (high)"));
        let stored = ctx.todos.lock().await.clone();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].status, "in_progress");
    }

    #[tokio::test]
    async fn normalizes_status_aliases() {
        let ctx = ctx();
        let out = TodoWriteTool
            .execute(
                &json!({"todos": [{"content": "a", "status": "done", "priority": "urgent"}]}),
                &ctx,
            )
            .await;
        assert!(out.content.contains("[x]"));
        assert!(out.content.contains("(high)"));
    }
}
