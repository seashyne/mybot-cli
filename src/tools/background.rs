//! Background process manager (`bg_start`, `bg_output`, `bg_stop`) for
//! long-running dev servers, watchers, and test daemons.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::bash::kill_process_group;
use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, arg_u64};

const MAX_BUFFER_LINES: usize = 500;
const DEFAULT_TAIL_LINES: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobStatus {
    Running,
    Exited(Option<i32>),
}

impl JobStatus {
    fn label(self) -> String {
        match self {
            Self::Running => "running".to_string(),
            Self::Exited(Some(code)) => format!("exited ({code})"),
            Self::Exited(None) => "exited (signal)".to_string(),
        }
    }
}

struct BgJob {
    command: String,
    pid: Option<u32>,
    lines: Arc<StdMutex<VecDeque<String>>>,
    status: Arc<StdMutex<JobStatus>>,
}

pub struct BackgroundManager {
    jobs: StdMutex<BTreeMap<String, BgJob>>,
    next_id: AtomicU64,
}

impl BackgroundManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            jobs: StdMutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
        })
    }

    fn start(&self, command: &str, ctx: &ToolCtx) -> Result<(String, Option<u32>), String> {
        let mut cmd = super::sandbox::build_command(command, &ctx.cwd, ctx.sandbox)?;
        let mut child = cmd
            .spawn()
            .map_err(|error| format!("cannot spawn background shell: {error}"))?;
        let pid = child.id();
        let id = format!("bg-{}", self.next_id.fetch_add(1, Ordering::Relaxed));

        let lines = Arc::new(StdMutex::new(VecDeque::with_capacity(64)));
        let status = Arc::new(StdMutex::new(JobStatus::Running));

        if let Some(stdout) = child.stdout.take() {
            let buf = lines.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    push_line(&buf, line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let buf = lines.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    push_line(&buf, format!("[stderr] {line}"));
                }
            });
        }

        let status_bg = status.clone();
        tokio::spawn(async move {
            let code = child.wait().await.ok().and_then(|s| s.code());
            if let Ok(mut guard) = status_bg.lock() {
                *guard = JobStatus::Exited(code);
            }
        });

        let job = BgJob {
            command: command.to_string(),
            pid,
            lines,
            status,
        };
        if let Ok(mut map) = self.jobs.lock() {
            map.insert(id.clone(), job);
        }
        Ok((id, pid))
    }

    fn list_summary(&self) -> String {
        let Ok(map) = self.jobs.lock() else {
            return "no background jobs".to_string();
        };
        if map.is_empty() {
            return "no background jobs".to_string();
        }
        let mut out = String::from("background jobs:\n");
        for (id, job) in map.iter() {
            let st = job
                .status
                .lock()
                .map(|g| g.label())
                .unwrap_or_else(|_| "unknown".into());
            out.push_str(&format!("  {id}  [{st}]  $ {}\n", job.command));
        }
        out.trim_end().to_string()
    }

    fn read_output(&self, id: &str, tail: usize) -> Result<String, String> {
        let map = self.jobs.lock().map_err(|_| "lock poisoned".to_string())?;
        let job = map
            .get(id)
            .ok_or_else(|| format!("unknown background job '{id}'"))?;
        let status = job
            .status
            .lock()
            .map(|g| g.label())
            .unwrap_or_else(|_| "unknown".into());
        let lines = job
            .lines
            .lock()
            .map(|buf| {
                let skip = buf.len().saturating_sub(tail);
                buf.iter().skip(skip).cloned().collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let body = if lines.is_empty() {
            "(no output yet)".to_string()
        } else {
            lines.join("\n")
        };
        Ok(format!("[{id} · {status} · $ {}]\n{body}", job.command))
    }

    fn stop(&self, id: &str) -> Result<String, String> {
        let mut map = self.jobs.lock().map_err(|_| "lock poisoned".to_string())?;
        let job = map
            .remove(id)
            .ok_or_else(|| format!("unknown background job '{id}'"))?;
        let was_running = job
            .status
            .lock()
            .map(|g| *g == JobStatus::Running)
            .unwrap_or(false);
        if was_running {
            kill_process_group(job.pid);
        }
        let tail = job
            .lines
            .lock()
            .map(|buf| {
                let skip = buf.len().saturating_sub(20);
                buf.iter().skip(skip).cloned().collect::<Vec<_>>().join("\n")
            })
            .unwrap_or_default();
        if tail.is_empty() {
            Ok(format!("stopped {id} ($ {})", job.command))
        } else {
            Ok(format!("stopped {id} ($ {})\n{tail}", job.command))
        }
    }
}

impl Drop for BackgroundManager {
    fn drop(&mut self) {
        if let Ok(map) = self.jobs.lock() {
            for job in map.values() {
                if job
                    .status
                    .lock()
                    .map(|g| *g == JobStatus::Running)
                    .unwrap_or(false)
                {
                    kill_process_group(job.pid);
                }
            }
        }
    }
}

fn push_line(buf: &Arc<StdMutex<VecDeque<String>>>, line: String) {
    if let Ok(mut guard) = buf.lock() {
        if guard.len() >= MAX_BUFFER_LINES {
            guard.pop_front();
        }
        guard.push_back(line);
    }
}

pub struct BgStartTool(pub Arc<BackgroundManager>);
pub struct BgOutputTool(pub Arc<BackgroundManager>);
pub struct BgStopTool(pub Arc<BackgroundManager>);

#[async_trait]
impl Tool for BgStartTool {
    fn name(&self) -> &str {
        "bg_start"
    }

    fn description(&self) -> &str {
        "Start a long-running shell command in the background (such as a dev server or watcher). Returns a job id for bg_output and bg_stop."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to run in the background"}
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(command) = arg_str(args, "command") else {
            return ToolOutput::error("missing required argument: command");
        };
        if command.trim().is_empty() {
            return ToolOutput::error("command must not be empty");
        }
        match self.0.start(command, ctx) {
            Ok((id, pid)) => ToolOutput::ok(format!(
                "started {id} (pid {}): {command}\nUse bg_output(id=\"{id}\") to read logs and bg_stop(id=\"{id}\") to stop.",
                pid.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
            )),
            Err(error) => ToolOutput::error(error),
        }
    }

    fn summary(&self, args: &Value) -> String {
        let cmd = arg_str(args, "command").unwrap_or_default();
        format!("bg $ {}", cmd.lines().next().unwrap_or_default())
    }
}

#[async_trait]
impl Tool for BgOutputTool {
    fn name(&self) -> &str {
        "bg_output"
    }

    fn description(&self) -> &str {
        "Read recent output and running status of a background job, or omit id to list all jobs."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "Job id returned by bg_start (omit to list jobs)"},
                "tail_lines": {"type": "integer", "description": "Number of recent lines to return (default 80)"}
            }
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let Some(id) = arg_str(args, "id").filter(|s| !s.trim().is_empty()) else {
            return ToolOutput::ok(self.0.list_summary());
        };
        let tail = arg_u64(args, "tail_lines")
            .unwrap_or(DEFAULT_TAIL_LINES as u64)
            .clamp(1, MAX_BUFFER_LINES as u64) as usize;
        match self.0.read_output(id, tail) {
            Ok(out) => ToolOutput::ok(out),
            Err(err) => ToolOutput::error(err),
        }
    }
}

#[async_trait]
impl Tool for BgStopTool {
    fn name(&self) -> &str {
        "bg_stop"
    }

    fn description(&self) -> &str {
        "Stop a background process started with bg_start."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "Job id returned by bg_start"}
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let Some(id) = arg_str(args, "id") else {
            return ToolOutput::error("missing required argument: id");
        };
        match self.0.stop(id) {
            Ok(out) => ToolOutput::ok(out),
            Err(err) => ToolOutput::error(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starts_reads_and_stops_background_job() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::for_test(dir.path().to_path_buf());
        let mgr = BackgroundManager::new();

        let started = BgStartTool(mgr.clone())
            .execute(&json!({"command": "echo bg_ready; sleep 10"}), &ctx)
            .await;
        assert!(!started.is_error, "{}", started.content);
        assert!(started.content.contains("bg-1"));

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let out = BgOutputTool(mgr.clone())
            .execute(&json!({"id": "bg-1"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("bg_ready"), "got {}", out.content);
        assert!(out.content.contains("running"), "got {}", out.content);

        let stopped = BgStopTool(mgr.clone())
            .execute(&json!({"id": "bg-1"}), &ctx)
            .await;
        assert!(!stopped.is_error, "{}", stopped.content);
        assert!(stopped.content.contains("stopped bg-1"));
    }
}
