use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::truncate::head_tail;
use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, arg_u64};

pub struct BashTool;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 300;
const PREVIEW_BYTES: usize = 22 * 1024;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run a shell command with bash in the working directory. Returns stdout and stderr. Commands are killed when the timeout expires."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to run"},
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default 120, max 300)"
                }
            },
            "required": ["command"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(command) = arg_str(args, "command") else {
            return ToolOutput::error("missing required argument: command");
        };
        if command.trim().is_empty() {
            return ToolOutput::error("command must not be empty");
        }

        let timeout = arg_u64(args, "timeout")
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS);

        let mut cmd = match super::sandbox::build_command(command, &ctx.cwd, ctx.sandbox) {
            Ok(cmd) => cmd,
            Err(error) => return ToolOutput::error(error),
        };

        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => return ToolOutput::error(format!("cannot spawn shell: {error}")),
        };
        let pid = child.id();

        let mut handle = tokio::spawn(async move { child.wait_with_output().await });
        let started = Instant::now();

        let outcome = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                kill_process_group(pid);
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut handle).await;
                return ToolOutput::error("command cancelled");
            }
            result = tokio::time::timeout(Duration::from_secs(timeout), &mut handle) => result,
        };

        let output = match outcome {
            Err(_elapsed) => {
                kill_process_group(pid);
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut handle).await;
                return ToolOutput::error(format!(
                    "command timed out after {timeout}s and was killed"
                ));
            }
            Ok(Err(join_error)) => {
                return ToolOutput::error(format!("command task failed: {join_error}"));
            }
            Ok(Ok(Err(io_error))) => {
                return ToolOutput::error(format!("command failed: {io_error}"));
            }
            Ok(Ok(Ok(output))) => output,
        };

        let elapsed = started.elapsed();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut combined = String::new();
        if !stdout.trim_end().is_empty() {
            combined.push_str(stdout.trim_end());
        }
        if !stderr.trim_end().is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str("[stderr]\n");
            combined.push_str(stderr.trim_end());
        }
        if combined.is_empty() {
            combined.push_str("(no output)");
        }

        let (mut text, truncated) = head_tail(&combined, PREVIEW_BYTES);
        if truncated
            && let Ok(mut file) = tempfile::NamedTempFile::new() {
                use std::io::Write;
                if file.write_all(combined.as_bytes()).is_ok()
                    && let Ok((_, path)) = file.keep() {
                        text.push_str(&format!("\n[full output: {}]", path.display()));
                    }
            }

        if let Some(code) = output.status.code() {
            if code != 0 {
                text.push_str(&format!("\n[exit code: {code}]"));
            }
        } else {
            text.push_str("\n[killed by signal]");
        }
        if elapsed.as_secs() >= 1 {
            text.push_str(&format!("\n[elapsed: {:.1}s]", elapsed.as_secs_f64()));
        }

        ToolOutput {
            content: text,
            is_error: !output.status.success(),
        }
    }

    fn summary(&self, args: &Value) -> String {
        let command = arg_str(args, "command").unwrap_or_default();
        let first_line = command.lines().next().unwrap_or_default();
        if first_line.chars().count() > 120 {
            format!("$ {}…", first_line.chars().take(120).collect::<String>())
        } else {
            format!("$ {first_line}")
        }
    }
}

#[cfg(unix)]
pub(crate) fn kill_process_group(pid: Option<u32>) {
    if let Some(pid) = pid {
        // Negative pid targets the whole process group created by process_group(0).
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
pub(crate) fn kill_process_group(pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn kill_process_group(_pid: Option<u32>) {}

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
    async fn captures_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let out = BashTool
            .execute(&json!({"command": "echo hello"}), &ctx(dir.path()))
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("hello"));
    }

    #[tokio::test]
    async fn captures_stderr_and_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let out = BashTool
            .execute(
                &json!({"command": "echo oops >&2; exit 3"}),
                &ctx(dir.path()),
            )
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("oops"));
        assert!(out.content.contains("exit code: 3"));
    }

    #[tokio::test]
    async fn runs_in_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("marker.txt"), "x").unwrap();
        let out = BashTool
            .execute(&json!({"command": "ls"}), &ctx(dir.path()))
            .await;
        assert!(out.content.contains("marker.txt"));
    }

    #[tokio::test]
    async fn times_out() {
        let dir = tempfile::tempdir().unwrap();
        let out = BashTool
            .execute(&json!({"command": "sleep 5", "timeout": 1}), &ctx(dir.path()))
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("timed out"));
    }
}
