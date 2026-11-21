use async_trait::async_trait;
use serde_json::{Value, json};

use super::truncate::truncate_line;
use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, arg_u64, display_path, resolve_path};

pub struct ReadTool;

const MAX_LINES: usize = 2000;
const MAX_BYTES: usize = 50 * 1024;
const MAX_LINE_CHARS: usize = 2000;
const BINARY_SNIFF: usize = 8192;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> &'static str {
        "Read a text file from the filesystem. Output is line-numbered (N→text). Use offset and limit to page through large files."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path: relative to the working directory, absolute, or ~/..."
                },
                "offset": {
                    "type": "integer",
                    "description": "1-based line number to start reading from"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return"
                }
            },
            "required": ["path"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(raw_path) = arg_str(args, "path") else {
            return ToolOutput::error("missing required argument: path");
        };
        let path = resolve_path(&ctx.cwd, raw_path);

        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return ToolOutput::error(format!("cannot read {}: {error}", path.display()));
            }
        };

        let sniff = &bytes[..bytes.len().min(BINARY_SNIFF)];
        if sniff.contains(&0) {
            return ToolOutput::error(format!(
                "{} looks like a binary file; refusing to read it as text",
                display_path(&ctx.cwd, &path)
            ));
        }

        let text = String::from_utf8_lossy(&bytes);
        let total_lines = text.lines().count();
        let offset = arg_u64(args, "offset").unwrap_or(1).max(1) as usize;
        let limit = arg_u64(args, "limit")
            .map(|value| value as usize)
            .unwrap_or(MAX_LINES)
            .min(MAX_LINES);

        if offset > total_lines && total_lines > 0 {
            return ToolOutput::ok(format!(
                "offset {offset} is past the end of the file ({total_lines} lines)"
            ));
        }

        let mut out = String::new();
        let mut bytes_written = 0usize;
        let mut shown = 0usize;
        let mut truncated = false;

        for (index, line) in text.lines().enumerate().skip(offset - 1) {
            if shown >= limit {
                truncated = true;
                break;
            }
            let (line, line_cut) = truncate_line(line, MAX_LINE_CHARS);
            let rendered = format!("{}→{line}\n", index + 1);
            if bytes_written + rendered.len() > MAX_BYTES {
                truncated = true;
                break;
            }
            if line_cut {
                truncated = true;
            }
            bytes_written += rendered.len();
            out.push_str(&rendered);
            shown += 1;
        }

        if out.is_empty() {
            return ToolOutput::ok(format!(
                "{} is empty",
                display_path(&ctx.cwd, &path)
            ));
        }

        if truncated {
            out.push_str(&format!(
                "\n[truncated: showed {shown} of {total_lines} lines; use offset/limit to read more]\n"
            ));
        }

        ToolOutput::ok(out)
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
        }
    }

    #[tokio::test]
    async fn reads_numbered_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let out = ReadTool
            .execute(&json!({"path": "a.txt"}), &ctx(dir.path()))
            .await;
        assert!(!out.is_error);
        assert!(out.content.contains("1→one"));
        assert!(out.content.contains("3→three"));
    }

    #[tokio::test]
    async fn honours_offset_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "1\n2\n3\n4\n5\n").unwrap();
        let out = ReadTool
            .execute(&json!({"path": "a.txt", "offset": 2, "limit": 2}), &ctx(dir.path()))
            .await;
        assert!(out.content.contains("2→2"));
        assert!(out.content.contains("3→3"));
        assert!(!out.content.contains("4→4"));
    }

    #[tokio::test]
    async fn rejects_binary() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.bin"), [0u8, 1, 2, 3]).unwrap();
        let out = ReadTool
            .execute(&json!({"path": "b.bin"}), &ctx(dir.path()))
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("binary"));
    }

    #[tokio::test]
    async fn missing_file_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let out = ReadTool
            .execute(&json!({"path": "nope.txt"}), &ctx(dir.path()))
            .await;
        assert!(out.is_error);
    }
}
