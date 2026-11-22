use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};

use super::config::McpServerConfig;

pub struct StdioTransport {
    name: String,
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: AtomicU64,
    timeout: Duration,
    _child: Mutex<Child>,
}

impl StdioTransport {
    pub async fn spawn(name: &str, cfg: &McpServerConfig) -> Result<Self, String> {
        let command = cfg
            .command
            .as_deref()
            .ok_or_else(|| "missing stdio command".to_string())?;

        let mut cmd = Command::new(command);
        cmd.args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in &cfg.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .map_err(|error| format!("cannot start '{command}': {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "server has no stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "server has no stdout".to_string())?;
        let stderr = child.stderr.take();

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let reader_pending = pending.clone();
        let reader_name = name.to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let Ok(value) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        if let Some(id) = value.get("id").and_then(Value::as_u64) {
                            let sender = reader_pending.lock().await.remove(&id);
                            if let Some(sender) = sender {
                                let _ = sender.send(value);
                            }
                        }
                        // Server-initiated notifications are ignored for now.
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            // Dropping the senders wakes every in-flight request with an error.
            reader_pending.lock().await.clear();
            let _ = reader_name;
        });

        if let Some(stderr) = stderr {
            let path = crate::config::data_dir().join(format!("mcp-{}.log", sanitize(name)));
            tokio::spawn(async move {
                use std::io::Write;
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .and_then(|mut file| writeln!(file, "{line}"));
                }
            });
        }

        Ok(Self {
            name: name.to_string(),
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicU64::new(1),
            timeout: Duration::from_secs(cfg.timeout_secs.max(1)),
            _child: Mutex::new(child),
        })
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = self.send_line(&message).await {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }

        match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(value)) => {
                if let Some(error) = value.get("error") {
                    return Err(format_error(error));
                }
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            }
            Ok(Err(_)) => Err(format!("{} closed its output before replying", self.name)),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(format!(
                    "{} did not reply within {}s",
                    self.name,
                    self.timeout.as_secs()
                ))
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.send_line(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    async fn send_line(&self, value: &Value) -> Result<(), String> {
        let mut line =
            serde_json::to_string(value).map_err(|error| format!("cannot encode request: {error}"))?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| format!("cannot write to {}: {error}", self.name))?;
        stdin
            .flush()
            .await
            .map_err(|error| format!("cannot flush {}: {error}", self.name))
    }
}

fn format_error(error: &Value) -> String {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("JSON-RPC error");
    match error.get("code").and_then(Value::as_i64) {
        Some(code) => format!("{message} (code {code})"),
        None => message.to_string(),
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
