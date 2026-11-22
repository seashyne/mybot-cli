use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentEvent, CompactOutcome};
use crate::permissions::Decision;
use crate::tools::TodoItem;

use super::composer::Composer;
use super::permission::PermissionModal;

mod images;
mod keys;
mod mention;
mod palette;
mod picker;
mod slash;
#[cfg(test)]
mod tests;

pub use mention::{Mention, MentionKind};
#[cfg(test)]
pub use mention::MentionItem;
pub use palette::Palette;
pub use picker::ModelPicker;
#[cfg(test)]
pub use picker::ModelItem;

pub struct Ctx {
    pub agent: Arc<Mutex<Agent>>,
    pub tx: mpsc::Sender<AgentEvent>,
    pub ui_tx: mpsc::UnboundedSender<UiEvent>,
    pub skills: Arc<crate::skills::SkillSet>,
    pub plugins: Arc<crate::plugins::PluginSet>,
    pub mcp: Arc<crate::mcp::McpManager>,
    pub agents: Arc<crate::subagents::AgentSet>,
    pub permissions: Arc<crate::permissions::PermissionEngine>,
    pub tools: Arc<crate::tools::ToolRegistry>,
    pub checkpoints: crate::tools::checkpoint::CheckpointStore,
}

/// Background work that updates the UI without going through the agent loop.
#[derive(Debug)]
pub enum UiEvent {
    Models {
        provider: String,
        models: Vec<String>,
        error: Option<String>,
    },
}

pub struct Status {
    pub provider: String,
    pub model: String,
    pub permission: crate::config::PermissionMode,
    pub thinking_budget: Option<u32>,
    pub session: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

pub enum Entry {
    User(String),
    Assistant(String),
    Tool {
        id: String,
        name: String,
        summary: String,
        content: String,
        is_error: bool,
        running: bool,
        expanded: bool,
    },
    Info(String),
    Error(String),
}

pub struct App {
    pub transcript: Vec<Entry>,
    pub streaming: String,
    pub streaming_active: bool,
    pub thinking_chars: usize,
    pub composer: Composer,
    pub pending_images: Vec<crate::protocol::ImageAttachment>,
    pub todos: Vec<TodoItem>,
    pub team: Option<crate::team::TeamSnapshot>,
    pub permission: Option<PermissionModal>,
    pub model_picker: Option<ModelPicker>,
    pub palette: Option<Palette>,
    pub mention: Option<Mention>,
    pub(crate) mention_dismissed: Option<(usize, usize)>,
    pub last_prompt: Option<String>,
    pub status: Status,
    pub step_output_tokens: u64,
    pub busy: bool,
    pub quit: bool,
    pub scroll_from_bottom: u16,
    pub auto_scroll: bool,
    pub info: Option<(String, Instant)>,
    pub spinner: usize,
    pub cwd: PathBuf,
    pub cancel: CancellationToken,
    pub pending_initial: Option<String>,
}

impl App {
    pub fn new(status: Status, cwd: PathBuf, initial_prompt: Option<String>) -> Self {
        Self {
            transcript: Vec::new(),
            streaming: String::new(),
            streaming_active: false,
            thinking_chars: 0,
            composer: Composer::new(),
            pending_images: Vec::new(),
            todos: Vec::new(),
            team: None,
            permission: None,
            model_picker: None,
            palette: None,
            mention: None,
            mention_dismissed: None,
            last_prompt: None,
            status,
            step_output_tokens: 0,
            busy: false,
            quit: false,
            scroll_from_bottom: 0,
            auto_scroll: true,
            info: None,
            spinner: 0,
            cwd,
            cancel: CancellationToken::new(),
            pending_initial: initial_prompt,
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.info = Some((message.into(), Instant::now()));
    }

    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
        if let Some((_, at)) = &self.info
            && at.elapsed() > Duration::from_secs(5) {
                self.info = None;
            }
    }

    pub fn start_initial(&mut self, ctx: &Ctx) {
        if let Some(prompt) = self.pending_initial.take() {
            self.composer.set_text(&prompt);
            self.submit(ctx);
        }
    }

    pub fn set_permission(
        &mut self,
        request: crate::permissions::PermissionRequest,
        reply: tokio::sync::oneshot::Sender<Decision>,
    ) {
        self.permission = Some(PermissionModal::new(request, reply));
    }

    pub fn handle_ui(&mut self, event: UiEvent) {
        match event {
            UiEvent::Models {
                provider,
                models,
                error,
            } => {
                if let Some(picker) = &mut self.model_picker {
                    picker.merge_models(&provider, &models);
                    picker.pending = picker.pending.saturating_sub(1);
                    if picker.pending == 0 {
                        picker.loading = false;
                    }
                    if let Some(error) = error {
                        picker.notes.push(format!("{provider}: {error}"));
                    }
                }
            }
        }
    }

    pub fn handle_agent(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStart { .. } => {
                self.streaming_active = true;
                self.thinking_chars = 0;
                self.step_output_tokens = 0;
            }
            AgentEvent::TextDelta(text) => self.streaming.push_str(&text),
            AgentEvent::ThinkingDelta(text) => self.thinking_chars += text.len(),
            AgentEvent::ToolCall {
                id,
                name,
                summary,
            } => {
                self.finish_streaming();
                self.transcript.push(Entry::Tool {
                    id,
                    name,
                    summary,
                    content: "queued".into(),
                    is_error: false,
                    running: true,
                    expanded: false,
                });
            }
            AgentEvent::ToolStart { id, .. } => {
                self.update_tool(&id, |content, _, _| {
                    *content = "running…".into();
                });
            }
            AgentEvent::ToolEnd {
                id,
                content,
                is_error,
                ..
            } => {
                self.update_tool(&id, |entry_content, entry_error, running| {
                    *entry_content = content;
                    *entry_error = is_error;
                    *running = false;
                });
            }
            AgentEvent::Usage { input, output } => {
                if input > 0 {
                    self.status.input_tokens = input;
                    self.status.total_input_tokens =
                        self.status.total_input_tokens.saturating_add(input);
                }
                if output > self.step_output_tokens {
                    let delta = output - self.step_output_tokens;
                    self.status.total_output_tokens =
                        self.status.total_output_tokens.saturating_add(delta);
                    self.step_output_tokens = output;
                    self.status.output_tokens = output;
                }
            }
            AgentEvent::TurnEnd { .. } => {
                self.finish_streaming();
            }
            AgentEvent::ModelName(name) => {
                if !name.is_empty() {
                    self.status.model = name;
                }
            }
            AgentEvent::Retry {
                attempt,
                max_retries,
                delay_ms,
                error,
            } => {
                let error = truncate(&error, 60);
                self.info(format!(
                    "retrying {attempt}/{max_retries} in {:.1}s — {error}",
                    delay_ms as f64 / 1000.0
                ));
            }
            AgentEvent::CompactStart { messages, tokens } => {
                self.info(format!(
                    "compacting {messages} messages (~{tokens} tokens)…"
                ));
            }
            AgentEvent::CompactEnd(outcome) => {
                self.busy = false;
                self.streaming_active = false;
                match outcome {
                    CompactOutcome::Compacted(report) => {
                        let suffix = if report.truncated {
                            " (summary truncated)"
                        } else {
                            ""
                        };
                        self.transcript.push(Entry::Info(format!(
                            "── compacted {} messages → summary · ~{} → ~{} tokens{} ──",
                            report.messages_before,
                            report.tokens_before,
                            report.tokens_after,
                            suffix
                        )));
                        self.info("conversation compacted");
                    }
                    CompactOutcome::Empty => self.info("nothing to compact yet"),
                    CompactOutcome::Cancelled => self.info("compaction cancelled"),
                }
            }
            AgentEvent::Done { .. } => {
                self.finish_streaming();
                self.busy = false;
                self.streaming_active = false;
            }
            AgentEvent::Todos(todos) => self.todos = todos,
            AgentEvent::Team(snapshot) => self.team = Some(snapshot),
            AgentEvent::Error(message) => {
                self.finish_streaming();
                self.transcript.push(Entry::Error(message));
                self.busy = false;
                self.streaming_active = false;
            }
        }
    }

    fn finish_streaming(&mut self) {
        let text = std::mem::take(&mut self.streaming);
        if !text.trim().is_empty() {
            self.transcript.push(Entry::Assistant(text));
        }
    }

    fn update_tool(
        &mut self,
        id: &str,
        update: impl FnOnce(&mut String, &mut bool, &mut bool),
    ) {
        for entry in self.transcript.iter_mut().rev() {
            if let Entry::Tool {
                id: entry_id,
                content,
                is_error,
                running,
                ..
            } = entry
                && entry_id == id {
                    update(content, is_error, running);
                    return;
                }
        }
    }
}

pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
