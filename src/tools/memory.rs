use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::memory::{MemoryKind, MemoryStore};

use super::{ExecutionMode, Tool, ToolCtx, ToolOutput};

/// Read and update the persistent project memory.
pub struct MemoryTool {
    /// Overrides the project-derived memory directory (used by tests).
    root: Option<PathBuf>,
}

impl MemoryTool {
    pub fn new() -> Self {
        Self { root: None }
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    fn store(&self, ctx: &ToolCtx) -> MemoryStore {
        match &self.root {
            Some(root) => MemoryStore::load_from(root.clone()),
            None => MemoryStore::load(&ctx.cwd),
        }
    }
}

impl Default for MemoryTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn description(&self) -> &'static str {
        "Read and update durable project memory. Use it to list entries, read one, save a decision or preference that should survive future sessions, or delete an outdated entry. Never store secrets, keys, or tokens."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "read", "write", "delete"],
                    "description": "list (default), read one entry, write/replace one entry, or delete one entry"
                },
                "name": {
                    "type": "string",
                    "description": "Entry id, lowercase words joined with '-' (required for read/write/delete)"
                },
                "title": {
                    "type": "string",
                    "description": "Short human title (write only; defaults to the name)"
                },
                "description": {
                    "type": "string",
                    "description": "One sentence describing when this memory matters (write only)"
                },
                "type": {
                    "type": "string",
                    "enum": ["user", "feedback", "project", "reference"],
                    "description": "Memory kind (write only; default project)"
                },
                "content": {
                    "type": "string",
                    "description": "Markdown body (write only)"
                }
            },
            "required": ["action"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("list")
            .trim()
            .to_ascii_lowercase();
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();

        let mut store = self.store(ctx);

        match action.as_str() {
            "list" => {
                let mut text = store.render_index();
                text.push_str(&format!("\nroot: {}", store.root.display()));
                if store.is_empty() {
                    text.push_str(
                        "\n\nsave one with action=write, name=<id>, content=<markdown>",
                    );
                }
                ToolOutput::ok(text)
            }
            "read" => {
                if name.is_empty() {
                    return ToolOutput::error("read requires a memory name");
                }
                match store.get(name) {
                    Some(entry) => {
                        let mut text = format!(
                            "# {} ({})\nname: {}\ndescription: {}\npath: {}\n\n{}",
                            entry.title,
                            entry.kind.as_str(),
                            entry.name,
                            entry.description,
                            entry.path.display(),
                            entry.body
                        );
                        if !text.ends_with('\n') {
                            text.push('\n');
                        }
                        ToolOutput::ok(text)
                    }
                    None => ToolOutput::error(format!(
                        "unknown memory '{name}' (available: {})",
                        names(&store)
                    )),
                }
            }
            "write" => {
                if name.is_empty() {
                    return ToolOutput::error("write requires a memory name");
                }
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if content.trim().is_empty() {
                    return ToolOutput::error("write requires non-empty content");
                }
                let title = args.get("title").and_then(Value::as_str).unwrap_or("");
                let description = args
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let kind = args
                    .get("type")
                    .and_then(Value::as_str)
                    .and_then(MemoryKind::parse)
                    .unwrap_or(MemoryKind::Project);
                match store.write(name, title, description, kind, content) {
                    Ok(path) => ToolOutput::ok(format!(
                        "saved memory '{}' to {}",
                        crate::memory::slugify(name),
                        path.display()
                    )),
                    Err(error) => ToolOutput::error(error.to_string()),
                }
            }
            "delete" => {
                if name.is_empty() {
                    return ToolOutput::error("delete requires a memory name");
                }
                match store.delete(name) {
                    Ok(true) => ToolOutput::ok(format!(
                        "deleted memory '{}'",
                        crate::memory::slugify(name)
                    )),
                    Ok(false) => ToolOutput::error(format!(
                        "unknown memory '{name}' (available: {})",
                        names(&store)
                    )),
                    Err(error) => ToolOutput::error(error.to_string()),
                }
            }
            other => ToolOutput::error(format!(
                "unknown action '{other}' (expected list, read, write, or delete)"
            )),
        }
    }

    fn summary(&self, args: &Value) -> String {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("list");
        match args.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => format!("memory {action} {name}"),
            _ => format!("memory {action}"),
        }
    }
}

fn names(store: &MemoryStore) -> String {
    if store.is_empty() {
        return "none".to_string();
    }
    store
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
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
            checkpoints: crate::tools::checkpoint::CheckpointStore::new(),
            sandbox: crate::config::SandboxMode::Auto,
        }
    }

    #[tokio::test]
    async fn write_then_read_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MemoryTool::with_root(dir.path());

        let output = tool
            .execute(
                &json!({
                    "action": "write",
                    "name": "auth",
                    "title": "Auth decision",
                    "description": "why sessions are stateless",
                    "type": "project",
                    "content": "Cookies are signed."
                }),
                &ctx(),
            )
            .await;
        assert!(!output.is_error, "{}", output.content);

        let output = tool
            .execute(&json!({"action": "read", "name": "auth"}), &ctx())
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("Cookies are signed."));
        assert!(output.content.contains("Auth decision"));
    }

    #[tokio::test]
    async fn list_shows_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MemoryTool::with_root(dir.path());
        tool.execute(
            &json!({"action": "write", "name": "note", "content": "body"}),
            &ctx(),
        )
        .await;

        let output = tool.execute(&json!({"action": "list"}), &ctx()).await;
        assert!(output.content.contains("note.md"));
        assert!(output.content.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn unknown_entry_lists_available_names() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MemoryTool::with_root(dir.path());
        tool.execute(
            &json!({"action": "write", "name": "kept", "content": "body"}),
            &ctx(),
        )
        .await;

        let output = tool
            .execute(&json!({"action": "read", "name": "missing"}), &ctx())
            .await;
        assert!(output.is_error);
        assert!(output.content.contains("kept"));
    }

    #[tokio::test]
    async fn write_requires_content() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MemoryTool::with_root(dir.path());
        let output = tool
            .execute(&json!({"action": "write", "name": "empty"}), &ctx())
            .await;
        assert!(output.is_error);
    }
}
