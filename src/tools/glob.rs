use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, arg_u64, display_path, resolve_path};

pub struct GlobTool;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2000;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "glob"
    }

    fn description(&self) -> &'static str {
        "Find files by glob pattern (e.g. **/*.rs, src/**/*.ts). Respects .gitignore."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern matched against paths relative to the search root"},
                "path": {"type": "string", "description": "Directory to search (default: working directory)"},
                "limit": {"type": "integer", "description": "Maximum results (default 200)"}
            },
            "required": ["pattern"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(pattern) = arg_str(args, "pattern") else {
            return ToolOutput::error("missing required argument: pattern");
        };
        let base = resolve_path(&ctx.cwd, arg_str(args, "path").unwrap_or("."));
        if !base.exists() {
            return ToolOutput::error(format!("path does not exist: {}", base.display()));
        }
        let limit = arg_u64(args, "limit")
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, MAX_LIMIT);

        let matcher = match globset::Glob::new(pattern) {
            Ok(glob) => glob.compile_matcher(),
            Err(error) => return ToolOutput::error(format!("invalid glob: {error}")),
        };

        let cwd = ctx.cwd.clone();
        let result = tokio::task::spawn_blocking(move || {
            let walker = ignore::WalkBuilder::new(&base)
                .hidden(false)
                .git_ignore(true)
                .build();
            let mut found = Vec::new();
            for entry in walker.flatten() {
                if found.len() >= limit {
                    break;
                }
                let path = entry.path();
                if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    continue;
                }
                let relative = path.strip_prefix(&base).unwrap_or(path);
                let relative_string = relative.to_string_lossy().replace('\\', "/");
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if matcher.is_match(&relative_string) || matcher.is_match(name) {
                    found.push(display_path(&cwd, path));
                }
            }
            found.sort();
            found
        })
        .await;

        match result {
            Ok(found) if found.is_empty() => ToolOutput::ok("No files matched".to_string()),
            Ok(found) => {
                let mut output = found.join("\n");
                if found.len() >= limit {
                    output.push_str(&format!("\n[stopped at limit: {limit} files]"));
                }
                ToolOutput::ok(output)
            }
            Err(join_error) => ToolOutput::error(format!("glob task failed: {join_error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    fn ctx(dir: &std::path::Path) -> ToolCtx {
        ToolCtx {
            cwd: dir.to_path_buf(),
            cancel: CancellationToken::new(),
            todos: Arc::new(Mutex::new(Vec::new())),
            checkpoints: crate::tools::checkpoint::CheckpointStore::new(),
            sandbox: crate::config::SandboxMode::Auto,
        }
    }

    #[tokio::test]
    async fn finds_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/deep")).unwrap();
        std::fs::write(dir.path().join("src/deep/a.rs"), "").unwrap();
        std::fs::write(dir.path().join("src/b.txt"), "").unwrap();
        let out = GlobTool
            .execute(&json!({"pattern": "**/*.rs"}), &ctx(dir.path()))
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("src/deep/a.rs"));
        assert!(!out.content.contains("b.txt"));
    }

    #[tokio::test]
    async fn honours_limit() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..5 {
            std::fs::write(dir.path().join(format!("f{index}.txt")), "").unwrap();
        }
        let out = GlobTool
            .execute(&json!({"pattern": "*.txt", "limit": 2}), &ctx(dir.path()))
            .await;
        assert!(out.content.contains("limit"));
    }
}
