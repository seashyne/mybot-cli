//! Lifecycle hooks (`pre_tool_use`, `post_tool_use`, `stop`) loaded from
//! `~/.mybot/hooks.toml` and `<project>/.mybot/hooks.toml`.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

const DEFAULT_TIMEOUT_SECS: u64 = 15;
const MAX_TIMEOUT_SECS: u64 = 120;
const MAX_HOOK_OUTPUT_BYTES: usize = 4096;

#[derive(Debug, Clone, Deserialize)]
pub struct HookRule {
    #[serde(default)]
    pub matcher: String,
    pub command: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct HookSet {
    #[serde(default)]
    pub pre_tool_use: Vec<HookRule>,
    #[serde(default)]
    pub post_tool_use: Vec<HookRule>,
    #[serde(default)]
    pub stop: Vec<HookRule>,
}

impl HookSet {
    /// Load and merge `~/.mybot/hooks.toml` and `<cwd>/.mybot/hooks.toml`.
    pub fn load(cwd: &Path) -> Self {
        let mut merged = Self::default();
        let user_path = crate::config::data_dir().join("hooks.toml");
        merged.merge_file(&user_path);
        let project_path = cwd.join(".mybot").join("hooks.toml");
        if project_path != user_path {
            merged.merge_file(&project_path);
        }
        merged
    }

    fn merge_file(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(parsed) = toml::from_str::<HookSet>(&text) else {
            return;
        };
        self.pre_tool_use.extend(parsed.pre_tool_use);
        self.post_tool_use.extend(parsed.post_tool_use);
        self.stop.extend(parsed.stop);
    }

    pub fn is_empty(&self) -> bool {
        self.pre_tool_use.is_empty() && self.post_tool_use.is_empty() && self.stop.is_empty()
    }

    pub fn total_count(&self) -> usize {
        self.pre_tool_use.len() + self.post_tool_use.len() + self.stop.len()
    }

    /// Run matching `pre_tool_use` hooks. A non-zero exit blocks the tool call.
    pub async fn run_pre_tool_use(
        &self,
        tool: &str,
        args: &Value,
        cwd: &Path,
    ) -> Result<(), String> {
        let payload = json!({
            "event": "pre_tool_use",
            "tool": tool,
            "args": args,
            "cwd": cwd.display().to_string(),
        });
        for rule in &self.pre_tool_use {
            if !matcher_matches(&rule.matcher, tool) {
                continue;
            }
            let outcome = run_hook_command(rule, "pre_tool_use", Some(tool), Some(args), &payload, cwd).await;
            if !outcome.allowed {
                let reason = if outcome.output.is_empty() {
                    format!("blocked by pre_tool_use hook: {}", rule.command)
                } else {
                    format!("blocked by pre_tool_use hook: {}", outcome.output)
                };
                return Err(reason);
            }
        }
        Ok(())
    }

    /// Run matching `post_tool_use` hooks and return any combined diagnostics.
    pub async fn run_post_tool_use(
        &self,
        tool: &str,
        args: &Value,
        output: &str,
        is_error: bool,
        cwd: &Path,
    ) -> Option<String> {
        let payload = json!({
            "event": "post_tool_use",
            "tool": tool,
            "args": args,
            "output": truncate_bytes(output, MAX_HOOK_OUTPUT_BYTES),
            "is_error": is_error,
            "cwd": cwd.display().to_string(),
        });
        let mut notes = Vec::new();
        for rule in &self.post_tool_use {
            if !matcher_matches(&rule.matcher, tool) {
                continue;
            }
            let outcome = run_hook_command(rule, "post_tool_use", Some(tool), Some(args), &payload, cwd).await;
            if !outcome.output.is_empty() {
                notes.push(outcome.output);
            }
        }
        if notes.is_empty() {
            None
        } else {
            Some(truncate_bytes(&notes.join("\n"), MAX_HOOK_OUTPUT_BYTES))
        }
    }

    /// Run `stop` hooks when an agent turn completes.
    pub async fn run_stop(&self, stop_reason: &str, cwd: &Path) {
        if self.stop.is_empty() {
            return;
        }
        let payload = json!({
            "event": "stop",
            "stop_reason": stop_reason,
            "cwd": cwd.display().to_string(),
        });
        for rule in &self.stop {
            let _ = run_hook_command(rule, "stop", None, None, &payload, cwd).await;
        }
    }
}

struct HookOutcome {
    allowed: bool,
    output: String,
}

async fn run_hook_command(
    rule: &HookRule,
    event: &str,
    tool: Option<&str>,
    args: Option<&Value>,
    payload: &Value,
    cwd: &Path,
) -> HookOutcome {
    let mut cmd = shell_command(&rule.command);
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("MYBOT_HOOK_EVENT", event);
    if let Some(tool) = tool {
        cmd.env("MYBOT_HOOK_TOOL", tool);
    }
    if let Some(args) = args {
        if let Some(path) = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .and_then(Value::as_str)
        {
            cmd.env("MYBOT_HOOK_PATH", path);
        }
        if let Some(command) = args.get("command").and_then(Value::as_str) {
            cmd.env("MYBOT_HOOK_COMMAND", command);
        }
    }

    let Ok(mut child) = cmd.spawn() else {
        return HookOutcome {
            allowed: true,
            output: String::new(),
        };
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes()).await;
    }

    let timeout = Duration::from_secs(
        rule.timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS),
    );
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let combined = match (stdout.is_empty(), stderr.is_empty()) {
                (true, true) => String::new(),
                (false, true) => stdout,
                (true, false) => stderr,
                (false, false) => format!("{stdout}\n{stderr}"),
            };
            HookOutcome {
                allowed: output.status.success(),
                output: truncate_bytes(&combined, MAX_HOOK_OUTPUT_BYTES),
            }
        }
        Ok(Err(error)) => HookOutcome {
            allowed: false,
            output: format!("hook failed: {error}"),
        },
        Err(_) => HookOutcome {
            allowed: false,
            output: format!("hook timed out after {}s", timeout.as_secs()),
        },
    }
}

fn shell_command(script: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", script]);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = tokio::process::Command::new("bash");
        cmd.args(["-lc", script]);
        cmd
    }
}

/// Match `"*"` / empty (all tools), pipe-separated names (`"edit|write"`), or
/// prefix wildcards (`"mcp__*"`).
pub fn matcher_matches(matcher: &str, tool: &str) -> bool {
    let trimmed = matcher.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return true;
    }
    trimmed.split('|').any(|part| {
        let part = part.trim();
        if let Some(prefix) = part.strip_suffix('*') {
            tool.starts_with(prefix)
        } else {
            part == tool
        }
    })
}

fn truncate_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_supports_wildcards_and_alternatives() {
        assert!(matcher_matches("", "bash"));
        assert!(matcher_matches("*", "edit"));
        assert!(matcher_matches("edit|write", "write"));
        assert!(!matcher_matches("edit|write", "read"));
        assert!(matcher_matches("mcp__*", "mcp__github__list"));
    }

    #[tokio::test]
    async fn pre_tool_use_blocks_on_nonzero_exit_and_post_returns_output() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".mybot");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("hooks.toml"),
            r#"
[[pre_tool_use]]
matcher = "write"
command = "echo 'no writes allowed' >&2; exit 2"

[[post_tool_use]]
matcher = "read"
command = "echo hook-saw-$MYBOT_HOOK_TOOL"
"#,
        )
        .unwrap();

        let hooks = HookSet::load(dir.path());
        assert_eq!(hooks.pre_tool_use.len(), 1);
        assert_eq!(hooks.post_tool_use.len(), 1);

        let err = hooks
            .run_pre_tool_use("write", &json!({"path": "a.txt"}), dir.path())
            .await
            .unwrap_err();
        assert!(err.contains("no writes allowed"), "got {err}");

        assert!(
            hooks
                .run_pre_tool_use("read", &json!({"path": "a.txt"}), dir.path())
                .await
                .is_ok()
        );

        let note = hooks
            .run_post_tool_use("read", &json!({"path": "a.txt"}), "ok", false, dir.path())
            .await
            .unwrap();
        assert!(note.contains("hook-saw-read"), "got {note}");
    }
}
