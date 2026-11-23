use async_trait::async_trait;
use serde_json::{Value, json};

use super::edit_match::{Edit, apply_edits};
use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, display_path, resolve_path};

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> &'static str {
        "Replace exact text in a file. All edits are applied atomically: every oldText must match exactly once (or use replace_all). Include enough surrounding context to be unique."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to edit"},
                "edits": {
                    "type": "array",
                    "description": "Ordered list of replacements",
                    "items": {
                        "type": "object",
                        "properties": {
                            "oldText": {"type": "string", "description": "Exact text to replace"},
                            "newText": {"type": "string", "description": "Replacement text"}
                        },
                        "required": ["oldText", "newText"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let edits = match parse_edits(args) {
            Ok(edits) => edits,
            Err(error) => return ToolOutput::error(error),
        };

        let Some(raw_path) = arg_str(args, "path").or_else(|| arg_str(args, "file_path")) else {
            return ToolOutput::error("missing required argument: path");
        };
        let path = resolve_path(&ctx.cwd, raw_path);

        let original = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(error) => {
                return ToolOutput::error(format!("cannot read {}: {error}", path.display()));
            }
        };

        let updated = match apply_edits(&original, &edits) {
            Ok(updated) => updated,
            Err(error) => return ToolOutput::error(error),
        };

        if updated == original {
            return ToolOutput::ok(format!(
                "No changes made to {}",
                display_path(&ctx.cwd, &path)
            ));
        }

        ctx.checkpoints
            .record_before_write(&path, display_path(&ctx.cwd, &path));

        if let Err(error) = tokio::fs::write(&path, &updated).await {
            return ToolOutput::error(format!("cannot write {}: {error}", path.display()));
        }

        ToolOutput::ok(format!(
            "Edited {} ({} edit{})",
            display_path(&ctx.cwd, &path),
            edits.len(),
            if edits.len() == 1 { "" } else { "s" }
        ))
    }

    fn summary(&self, args: &Value) -> String {
        let path = arg_str(args, "path")
            .or_else(|| arg_str(args, "file_path"))
            .unwrap_or_default();
        format!("edit {path}")
    }
}

/// Accept `{path, edits:[...]}` as well as top-level `{path|file_path,
/// oldText|old_string, newText|new_string, replace_all|replaceAll}` shapes used
/// across coding models.
fn parse_edits(args: &Value) -> Result<Vec<Edit>, String> {
    if let Some(list) = args.get("edits").and_then(Value::as_array) {
        if list.is_empty() {
            return Err("edits must not be empty".into());
        }
        let mut edits = Vec::with_capacity(list.len());
        for (index, entry) in list.iter().enumerate() {
            let old = entry
                .get("oldText")
                .or_else(|| entry.get("old_string"))
                .and_then(Value::as_str)
                .ok_or_else(|| format!("edits[{index}] is missing oldText"))?;
            let new = entry
                .get("newText")
                .or_else(|| entry.get("new_string"))
                .and_then(Value::as_str)
                .ok_or_else(|| format!("edits[{index}] is missing newText"))?;
            let replace_all = entry
                .get("replace_all")
                .or_else(|| entry.get("replaceAll"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            edits.push(Edit {
                old: old.to_string(),
                new: new.to_string(),
                replace_all,
            });
        }
        return Ok(edits);
    }

    if let (Some(old), Some(new)) = (
        args.get("oldText")
            .or_else(|| args.get("old_string"))
            .and_then(Value::as_str),
        args.get("newText")
            .or_else(|| args.get("new_string"))
            .and_then(Value::as_str),
    ) {
        return Ok(vec![Edit {
            old: old.to_string(),
            new: new.to_string(),
            replace_all: args
                .get("replace_all")
                .or_else(|| args.get("replaceAll"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }]);
    }

    Err("expected {path, edits:[{oldText,newText}]} or {path, oldText, newText}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &std::path::Path) -> ToolCtx {
        ToolCtx::for_test(dir.to_path_buf())
    }

    #[tokio::test]
    async fn edits_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").unwrap();
        let out = EditTool
            .execute(
                &json!({"path": "a.rs", "edits": [{"oldText": "main", "newText": "run"}]}),
                &ctx(dir.path()),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
            "fn run() {}\n"
        );
    }

    #[tokio::test]
    async fn supports_legacy_shape() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
        let out = EditTool
            .execute(
                &json!({"file_path": "a.txt", "old_string": "world", "new_string": "there"}),
                &ctx(dir.path()),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "hello there\n"
        );
    }

    #[tokio::test]
    async fn ambiguous_edit_fails_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x\nx\n").unwrap();
        let out = EditTool
            .execute(
                &json!({"path": "a.txt", "edits": [{"oldText": "x", "newText": "y"}]}),
                &ctx(dir.path()),
            )
            .await;
        assert!(out.is_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "x\nx\n"
        );
    }
}
