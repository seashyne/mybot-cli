pub mod bash;
pub mod edit;
pub mod edit_match;
pub mod glob;
pub mod grep;
pub mod read;
pub mod todo;
pub mod truncate;
pub mod write;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::providers::ToolSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Safe, read-only tools may run concurrently.
    Parallel,
    /// Tools with side effects run one at a time, in call order.
    Sequential,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Clone)]
pub struct ToolCtx {
    pub cwd: PathBuf,
    pub cancel: CancellationToken,
    pub todos: Arc<Mutex<Vec<TodoItem>>>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> Value;
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }
    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput;

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.schema(),
        }
    }

    /// One-line human summary used by permission prompts and the transcript.
    fn summary(&self, args: &Value) -> String {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if path.is_empty() {
            self.name().to_string()
        } else {
            format!("{} {path}", self.name())
        }
    }

    /// File path touched by this call, if any (used for path permission checks).
    fn path_arg(&self, args: &Value) -> Option<String> {
        args.get("path")
            .and_then(Value::as_str)
            .map(str::to_string)
    }
}

pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(read::ReadTool));
        registry.register(Arc::new(write::WriteTool));
        registry.register(Arc::new(edit::EditTool));
        registry.register(Arc::new(bash::BashTool));
        registry.register(Arc::new(glob::GlobTool));
        registry.register(Arc::new(grep::GrepTool));
        registry.register(Arc::new(todo::TodoWriteTool));
        registry
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(String::as_str).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Resolve `~`, absolute, and relative paths against the working directory.
pub fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| cwd.to_path_buf());
    }
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Render a path relative to the working directory when possible.
pub fn display_path(cwd: &Path, path: &Path) -> String {
    match path.strip_prefix(cwd) {
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

pub fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

pub fn arg_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}
