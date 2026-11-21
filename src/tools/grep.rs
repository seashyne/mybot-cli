use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, arg_u64, display_path, resolve_path};

pub struct GrepTool;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search file contents with a regular expression. Respects .gitignore, skips binary files, and returns path:line:text matches."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regular expression (or literal string when literal=true)"},
                "path": {"type": "string", "description": "File or directory to search (default: working directory)"},
                "glob": {"type": "string", "description": "Only search files matching this glob, e.g. *.rs"},
                "ignoreCase": {"type": "boolean", "description": "Case-insensitive match"},
                "literal": {"type": "boolean", "description": "Treat pattern as a literal string"},
                "context": {"type": "integer", "description": "Lines of context around each match (max 10)"},
                "limit": {"type": "integer", "description": "Maximum matches to return (default 100)"}
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
        if pattern.is_empty() {
            return ToolOutput::error("pattern must not be empty");
        }

        let base = resolve_path(&ctx.cwd, arg_str(args, "path").unwrap_or("."));
        if !base.exists() {
            return ToolOutput::error(format!("path does not exist: {}", base.display()));
        }

        let ignore_case = args
            .get("ignoreCase")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let literal = args
            .get("literal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let context = arg_u64(args, "context").unwrap_or(0).min(10) as usize;
        let limit = arg_u64(args, "limit")
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, MAX_LIMIT);

        let expression = if literal {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };
        let regex = match regex::RegexBuilder::new(&expression)
            .case_insensitive(ignore_case)
            .build()
        {
            Ok(regex) => regex,
            Err(error) => return ToolOutput::error(format!("invalid pattern: {error}")),
        };

        let glob = match arg_str(args, "glob") {
            Some(pattern) => match globset::Glob::new(pattern) {
                Ok(glob) => Some(glob.compile_matcher()),
                Err(error) => return ToolOutput::error(format!("invalid glob: {error}")),
            },
            None => None,
        };

        let cwd = ctx.cwd.clone();
        let result = tokio::task::spawn_blocking(move || {
            search(&base, &cwd, &regex, glob.as_ref(), context, limit)
        })
        .await;

        match result {
            Ok(Ok(text)) => ToolOutput::ok(text),
            Ok(Err(error)) => ToolOutput::error(error),
            Err(join_error) => ToolOutput::error(format!("search task failed: {join_error}")),
        }
    }
}

fn search(
    base: &std::path::Path,
    cwd: &std::path::Path,
    regex: &regex::Regex,
    glob: Option<&globset::GlobMatcher>,
    context: usize,
    limit: usize,
) -> Result<String, String> {
    let walker = ignore::WalkBuilder::new(base)
        .hidden(false)
        .git_ignore(true)
        .build();

    let mut output = String::new();
    let mut matches = 0usize;
    let mut files_scanned = 0usize;

    for entry in walker.flatten() {
        if matches >= limit || output.len() >= MAX_OUTPUT_BYTES {
            break;
        }
        let path = entry.path();
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if let Some(matcher) = glob {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !matcher.is_match(name) && !matcher.is_match(path) {
                continue;
            }
        }
        if std::fs::metadata(path)
            .map(|meta| meta.len() > MAX_FILE_BYTES)
            .unwrap_or(true)
        {
            continue;
        }

        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes[..bytes.len().min(8192)].contains(&0) {
            continue;
        }
        files_scanned += 1;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        let rel = display_path(cwd, path);

        for (index, line) in lines.iter().enumerate() {
            if matches >= limit || output.len() >= MAX_OUTPUT_BYTES {
                break;
            }
            if !regex.is_match(line) {
                continue;
            }
            matches += 1;
            let start = index.saturating_sub(context);
            let end = (index + context + 1).min(lines.len());
            for (offset, context_line) in lines[start..end].iter().enumerate() {
                let line_number = start + offset + 1;
                let separator = if start + offset == index { ":" } else { "-" };
                output.push_str(&format!("{rel}{separator}{line_number}{separator}{context_line}\n"));
                if output.len() >= MAX_OUTPUT_BYTES {
                    break;
                }
            }
        }
    }

    if matches == 0 {
        return Ok(format!(
            "No matches found (searched {files_scanned} files)"
        ));
    }

    if matches >= limit {
        output.push_str(&format!("\n[stopped at limit: {limit} matches]\n"));
    } else if output.len() >= MAX_OUTPUT_BYTES {
        output.push_str("\n[output truncated at 16 KB]\n");
    }

    Ok(output)
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
    async fn finds_matches_with_line_numbers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\nlet x = 1;\n").unwrap();
        let out = GrepTool
            .execute(&json!({"pattern": "let x"}), &ctx(dir.path()))
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("a.rs:2:let x = 1;"));
    }

    #[tokio::test]
    async fn literal_mode_escapes_regex() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a.b\n").unwrap();
        let out = GrepTool
            .execute(&json!({"pattern": "a.b", "literal": true}), &ctx(dir.path()))
            .await;
        assert!(out.content.contains("a.txt"));
    }

    #[tokio::test]
    async fn glob_filters_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "needle\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "needle\n").unwrap();
        let out = GrepTool
            .execute(&json!({"pattern": "needle", "glob": "*.rs"}), &ctx(dir.path()))
            .await;
        assert!(out.content.contains("a.rs"));
        assert!(!out.content.contains("b.txt"));
    }

    #[tokio::test]
    async fn reports_no_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "nothing\n").unwrap();
        let out = GrepTool
            .execute(&json!({"pattern": "zzz"}), &ctx(dir.path()))
            .await;
        assert!(out.content.contains("No matches"));
    }
}
