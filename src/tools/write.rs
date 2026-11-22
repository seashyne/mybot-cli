use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, display_path, resolve_path};

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Write content to a file, creating parent directories as needed. Overwrites existing files."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Destination file path"
                },
                "content": {
                    "type": "string",
                    "description": "Full file content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(raw_path) = arg_str(args, "path") else {
            return ToolOutput::error("missing required argument: path");
        };
        let Some(content) = arg_str(args, "content") else {
            return ToolOutput::error("missing required argument: content");
        };
        let path = resolve_path(&ctx.cwd, raw_path);
        let existed = path.exists();

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
                && let Err(error) = tokio::fs::create_dir_all(parent).await {
                    return ToolOutput::error(format!(
                        "cannot create directory {}: {error}",
                        parent.display()
                    ));
                }

        ctx.checkpoints
            .record_before_write(&path, display_path(&ctx.cwd, &path));

        if let Err(error) = tokio::fs::write(&path, content).await {
            return ToolOutput::error(format!("cannot write {}: {error}", path.display()));
        }

        ToolOutput::ok(format!(
            "Wrote {} bytes to {} ({})",
            content.len(),
            display_path(&ctx.cwd, &path),
            if existed { "overwritten" } else { "created" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &std::path::Path) -> ToolCtx {
        ToolCtx::for_test(dir.to_path_buf())
    }

    #[tokio::test]
    async fn creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let out = WriteTool
            .execute(
                &json!({"path": "a/b/c.txt", "content": "hello"}),
                &ctx(dir.path()),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let written = std::fs::read_to_string(dir.path().join("a/b/c.txt")).unwrap();
        assert_eq!(written, "hello");
        assert!(out.content.contains("created"));
    }

    #[tokio::test]
    async fn reports_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.txt"), "old").unwrap();
        let out = WriteTool
            .execute(&json!({"path": "x.txt", "content": "new"}), &ctx(dir.path()))
            .await;
        assert!(out.content.contains("overwritten"));
    }
}
