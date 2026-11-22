use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentEvent, CompactOutcome, ProviderSwap};
use crate::config::{Config, PermissionMode};
use crate::permissions::Decision;
use crate::providers;
use crate::tools::TodoItem;

use super::composer::Composer;
use super::permission::PermissionModal;

pub struct Ctx {
    pub agent: Arc<Mutex<Agent>>,
    pub tx: mpsc::Sender<AgentEvent>,
    pub skills: Arc<crate::skills::SkillSet>,
    pub plugins: Arc<crate::plugins::PluginSet>,
}

pub struct ModelItem {
    pub provider: String,
    pub model: String,
    pub ready: bool,
}

pub struct ModelPicker {
    pub items: Vec<ModelItem>,
    pub selected: usize,
    /// `Some` while the user types a model name that is not in the list.
    pub custom: Option<String>,
    pub error: Option<String>,
}

impl ModelPicker {
    pub fn from_config(config: &Config) -> Self {
        let items = config
            .providers
            .iter()
            .map(|(name, provider)| {
                let model = provider
                    .model
                    .clone()
                    .unwrap_or_else(|| provider.kind.default_model().to_string());
                let ready = config
                    .resolve(Some(&format!("{name}/{model}")))
                    .is_ok();
                ModelItem {
                    provider: name.clone(),
                    model,
                    ready,
                }
            })
            .collect();
        Self {
            items,
            selected: 0,
            custom: None,
            error: None,
        }
    }
}

pub struct Status {
    pub provider: String,
    pub model: String,
    pub permission: PermissionMode,
    pub session: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
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
    pub todos: Vec<TodoItem>,
    pub permission: Option<PermissionModal>,
    pub model_picker: Option<ModelPicker>,
    pub status: Status,
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
            todos: Vec::new(),
            permission: None,
            model_picker: None,
            status,
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

    pub fn set_permission(&mut self, request: crate::permissions::PermissionRequest, reply: tokio::sync::oneshot::Sender<Decision>) {
        self.permission = Some(PermissionModal::new(request, reply));
    }

    pub fn handle_agent(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStart { .. } => {
                self.streaming_active = true;
                self.thinking_chars = 0;
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
                self.status.input_tokens = input;
                self.status.output_tokens = output;
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

    pub fn handle_paste(&mut self, text: &str) {
        if let Some(picker) = &mut self.model_picker {
            if let Some(buffer) = &mut picker.custom {
                buffer.push_str(text.trim());
            }
            return;
        }
        if self.permission.is_none() {
            self.composer.insert_str(text);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx) {
        if self.handle_model_key(key, ctx) {
            return;
        }
        if self.handle_permission_key(key) {
            return;
        }

        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            KeyCode::Char('c') if control => {
                if self.busy {
                    self.cancel.cancel();
                    self.info("cancelling…");
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('d') if control => {
                if self.composer.is_empty() {
                    self.quit = true;
                }
            }
            KeyCode::Esc => {
                if self.busy {
                    self.cancel.cancel();
                    self.info("cancelling…");
                } else {
                    self.composer.clear();
                }
            }
            KeyCode::Enter if shift || alt => self.composer.newline(),
            KeyCode::Char('j') if control => self.composer.newline(),
            KeyCode::Enter => self.submit(ctx),
            KeyCode::Backspace => self.composer.backspace(),
            KeyCode::Delete => self.composer.delete(),
            KeyCode::Left => self.composer.left(),
            KeyCode::Right => self.composer.right(),
            KeyCode::Up => {
                if self.composer.is_empty() {
                    self.composer.history_prev();
                } else {
                    self.composer.up();
                }
            }
            KeyCode::Down => {
                if self.composer.is_empty() {
                    self.composer.history_next();
                } else {
                    self.composer.down();
                }
            }
            KeyCode::Home => self.composer.home(),
            KeyCode::End => self.composer.end(),
            KeyCode::Tab => self.composer.insert_str("  "),
            KeyCode::PageUp => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(5);
                self.auto_scroll = false;
            }
            KeyCode::PageDown => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(5);
                if self.scroll_from_bottom == 0 {
                    self.auto_scroll = true;
                }
            }
            KeyCode::Char(value) if !control && !alt => self.composer.insert_char(value),
            _ => {}
        }
    }

    fn handle_permission_key(&mut self, key: KeyEvent) -> bool {
        if self.permission.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Char('a') => self.answer_permission(Decision::AllowOnce),
            KeyCode::Char('A') => self.answer_permission(Decision::AllowAlways),
            KeyCode::Char('d') | KeyCode::Esc => self.answer_permission(Decision::Deny),
            _ => {}
        }
        true
    }

    fn answer_permission(&mut self, decision: Decision) {
        if let Some(modal) = self.permission.take() {
            modal.answer(decision);
        }
    }

    fn handle_model_key(&mut self, key: KeyEvent, ctx: &Ctx) -> bool {
        if self.model_picker.is_none() {
            return false;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);

        // Read the pending action first so the picker is not borrowed twice.
        enum Act {
            None,
            Close,
            StopEditing,
            Move(isize),
            StartCustom,
            Edit(Option<char>),
            Apply(String, String),
        }

        let act = {
            let picker = self.model_picker.as_ref().unwrap();
            let editing = picker.custom.is_some();
            match key.code {
                KeyCode::Esc => {
                    if editing {
                        Act::StopEditing
                    } else {
                        Act::Close
                    }
                }
                KeyCode::Char('c') if control => Act::Close,
                KeyCode::Up | KeyCode::Char('k') if !editing => Act::Move(-1),
                KeyCode::Down | KeyCode::Char('j') if !editing => Act::Move(1),
                KeyCode::Backspace if editing => Act::Edit(None),
                KeyCode::Char(ch) if editing && !control => Act::Edit(Some(ch)),
                KeyCode::Enter => {
                    if let Some(buffer) = &picker.custom {
                        let model = buffer.trim().to_string();
                        if model.is_empty() {
                            Act::None
                        } else {
                            let provider = picker
                                .items
                                .get(picker.selected)
                                .map(|item| item.provider.clone())
                                .unwrap_or_else(|| self.status.provider.clone());
                            Act::Apply(provider, model)
                        }
                    } else if picker.selected < picker.items.len() {
                        let item = &picker.items[picker.selected];
                        Act::Apply(item.provider.clone(), item.model.clone())
                    } else {
                        Act::StartCustom
                    }
                }
                _ => Act::None,
            }
        };

        match act {
            Act::None => {}
            Act::Close => {
                self.model_picker = None;
                self.info("model unchanged");
            }
            Act::StopEditing => {
                if let Some(picker) = &mut self.model_picker {
                    picker.custom = None;
                }
            }
            Act::Move(delta) => {
                if let Some(picker) = &mut self.model_picker {
                    let max = picker.items.len();
                    let current = picker.selected as isize + delta;
                    picker.selected = current.clamp(0, max as isize) as usize;
                    picker.error = None;
                }
            }
            Act::StartCustom => {
                if let Some(picker) = &mut self.model_picker {
                    picker.custom = Some(String::new());
                }
            }
            Act::Edit(Some(ch)) => {
                if let Some(picker) = &mut self.model_picker
                    && let Some(buffer) = &mut picker.custom
                {
                    buffer.push(ch);
                }
            }
            Act::Edit(None) => {
                if let Some(picker) = &mut self.model_picker
                    && let Some(buffer) = &mut picker.custom
                {
                    buffer.pop();
                }
            }
            Act::Apply(provider, model) => self.apply_model(ctx, &provider, &model),
        }
        true
    }

    fn apply_model(&mut self, ctx: &Ctx, provider: &str, model: &str) {
        let attempt = (|| -> crate::error::Result<()> {
            let mut config = Config::load()?;
            let entry = config.providers.get_mut(provider).ok_or_else(|| {
                crate::error::AppError::config(format!("unknown provider '{provider}'"))
            })?;
            entry.model = Some(model.to_string());
            config.default_provider = Some(provider.to_string());

            let resolved = config.resolve(Some(&format!("{provider}/{model}")))?;
            let built: Arc<dyn crate::providers::Provider> =
                Arc::from(providers::build(&resolved)?);
            config.save()?;

            let swap = ProviderSwap {
                provider: built,
                max_tokens: resolved.max_tokens,
                temperature: resolved.temperature,
                thinking_budget: resolved.thinking_budget,
                retry: providers::RetryPolicy::from_resolved(&resolved),
            };
            let agent = ctx.agent.clone();
            tokio::spawn(async move {
                agent.lock().await.set_provider(swap);
            });
            Ok(())
        })();

        match attempt {
            Ok(()) => {
                self.status.provider = provider.to_string();
                self.status.model = model.to_string();
                self.model_picker = None;
                self.info(format!("model: {provider}/{model}"));
                self.transcript.push(Entry::Info(format!(
                    "switched to {provider}/{model} (saved to config)"
                )));
            }
            Err(error) => {
                if let Some(picker) = &mut self.model_picker {
                    picker.error = Some(error.to_string());
                }
            }
        }
    }

    fn submit(&mut self, ctx: &Ctx) {
        if self.busy {
            self.info("wait for the current turn to finish (Esc cancels)");
            return;
        }

        let text = self.composer.take();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        if trimmed.starts_with('/') {
            self.slash(trimmed, ctx);
            return;
        }

        self.transcript.push(Entry::User(trimmed.to_string()));
        self.start_turn(trimmed.to_string(), ctx);
    }

    /// Send `prompt` to the model without touching the composer.
    fn start_turn(&mut self, prompt: String, ctx: &Ctx) {
        if self.busy {
            self.info("wait for the current turn to finish (Esc cancels)");
            return;
        }

        self.busy = true;
        self.streaming_active = true;
        self.streaming.clear();
        self.thinking_chars = 0;
        self.cancel = CancellationToken::new();
        self.auto_scroll = true;
        self.scroll_from_bottom = 0;

        let agent = ctx.agent.clone();
        let tx = ctx.tx.clone();
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut agent = agent.lock().await;
            // Errors are reported through AgentEvent::Error.
            let _ = agent.run_turn(&prompt, &tx, &cancel).await;
        });
    }

    fn slash(&mut self, command: &str, ctx: &Ctx) {
        let (name, args) = command.split_once(' ').unwrap_or((command, ""));
        let args = args.trim();

        match name {
            "/quit" | "/exit" => self.quit = true,
            "/help" => {
                self.transcript.push(Entry::Info(
                    "commands: /help /new /model /skills /skill /sessions /status /compact /init /quit\n\
                     plugin commands: any /<name> listed by /skills\n\
                     keys: Enter send · Shift+Enter newline · Esc cancel/clear · Ctrl+C cancel/exit · Ctrl+D exit · PgUp/PgDn scroll"
                        .into(),
                ));
            }
            "/new" | "/clear" => {
                self.transcript.clear();
                self.streaming.clear();
                self.streaming_active = false;
                self.todos.clear();
                match crate::session::Session::create(
                    &self.cwd,
                    &self.status.provider,
                    &self.status.model,
                ) {
                    Ok(session) => {
                        self.status.session = short_id(session.id());
                        let session = Arc::new(Mutex::new(session));
                        let agent = ctx.agent.clone();
                        tokio::spawn(async move {
                            agent.lock().await.reset(Some(session)).await;
                        });
                        self.info("started a new session");
                    }
                    Err(error) => self.info(format!("cannot create session: {error}")),
                }
            }
            "/model" => match Config::load() {
                Ok(config) => {
                    let picker = ModelPicker::from_config(&config);
                    if picker.items.is_empty() {
                        self.info("no providers configured");
                    } else {
                        self.model_picker = Some(picker);
                    }
                }
                Err(error) => self.info(format!("cannot read config: {error}")),
            },
            "/sessions" => match crate::session::Session::list() {
                Ok(sessions) if sessions.is_empty() => self.info("no sessions yet"),
                Ok(sessions) => {
                    let mut text = String::from("recent sessions:\n");
                    for header in sessions.iter().take(10) {
                        text.push_str(&format!(
                            "  {}  {}  {}\n",
                            short_id(&header.id),
                            header.created_at.format("%Y-%m-%d %H:%M"),
                            header.model
                        ));
                    }
                    text.push_str("resume with `mybot --session <id>` or `mybot -c`");
                    self.transcript.push(Entry::Info(text));
                }
                Err(error) => self.info(format!("cannot list sessions: {error}")),
            },
            "/status" => {
                self.transcript.push(Entry::Info(format!(
                    "provider: {}\nmodel: {}\npermission: {}\nsession: {}\ntokens: ↑{} ↓{}",
                    self.status.provider,
                    self.status.model,
                    self.status.permission.label(),
                    self.status.session,
                    self.status.input_tokens,
                    self.status.output_tokens
                )));
            }
            "/skills" => {
                let mut text = String::new();
                if ctx.skills.is_empty() {
                    text.push_str("no skills installed\n");
                    text.push_str("add ~/.mybot/skills/<name>/SKILL.md or a plugin with skills/\n");
                } else {
                    text.push_str(&format!("skills ({}):\n", ctx.skills.len()));
                    for skill in &ctx.skills.skills {
                        text.push_str(&format!(
                            "  {:<18} {:<10} {}\n",
                            skill.name,
                            skill.source.label(),
                            skill.description
                        ));
                    }
                    text.push_str("load one with `skill` tool, or /skill <name>\n");
                }
                if !ctx.plugins.commands.is_empty() {
                    text.push_str(&format!("\nplugin commands ({}):\n", ctx.plugins.commands.len()));
                    for command in &ctx.plugins.commands {
                        text.push_str(&format!(
                            "  /{:<16} {:<12} {}\n",
                            command.name, command.plugin, command.description
                        ));
                    }
                }
                self.transcript.push(Entry::Info(text.trim_end().to_string()));
            }
            "/skill" => {
                if args.is_empty() {
                    self.info("usage: /skill <name>");
                    return;
                }
                match ctx.skills.get(args) {
                    Some(skill) => {
                        let prompt = format!(
                            "Follow the skill `{}` for this request.\n\n{}",
                            skill.name,
                            skill.body.trim()
                        );
                        self.transcript
                            .push(Entry::Info(format!("▸ skill {}", skill.name)));
                        self.start_turn(prompt, ctx);
                    }
                    None => self.info(format!("unknown skill '{args}' (try /skills)")),
                }
            }
            "/compact" => {
                self.busy = true;
                self.streaming_active = false;
                self.streaming.clear();
                self.thinking_chars = 0;
                self.cancel = CancellationToken::new();
                self.auto_scroll = true;
                self.scroll_from_bottom = 0;

                let agent = ctx.agent.clone();
                let tx = ctx.tx.clone();
                let cancel = self.cancel.clone();

                tokio::spawn(async move {
                    let mut agent = agent.lock().await;
                    // Failures are reported through AgentEvent::Error.
                    let _ = agent.compact(&tx, &cancel).await;
                });
                self.info("compacting…");
            }
            "/init" => {
                let path = self.cwd.join("AGENTS.md");
                if path.exists() {
                    self.info(format!("{} already exists", path.display()));
                } else {
                    let template = "# Project instructions\n\n- Build:\n- Test:\n- Conventions:\n";
                    match std::fs::write(&path, template) {
                        Ok(()) => self.info(format!("created {}", path.display())),
                        Err(error) => self.info(format!("cannot write AGENTS.md: {error}")),
                    }
                }
            }
            other => {
                let command_name = other.trim_start_matches('/');
                if let Some(command) = ctx.plugins.command(command_name) {
                    let rendered = command.render(args);
                    let label = if args.is_empty() {
                        format!("▸ /{command_name}")
                    } else {
                        format!("▸ /{command_name} {args}")
                    };
                    self.transcript.push(Entry::Info(label));
                    self.start_turn(rendered, ctx);
                } else {
                    self.info(format!("unknown command {other} (try /help)"));
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{CompactOutcome, CompactReport};
    use crate::protocol::StopReason;

    fn app() -> App {
        App::new(
            Status {
                provider: "mock".into(),
                model: "mock-model".into(),
                permission: PermissionMode::Auto,
                session: "abc12345".into(),
                input_tokens: 0,
                output_tokens: 0,
            },
            PathBuf::from("/tmp"),
            None,
        )
    }

    #[test]
    fn streaming_becomes_assistant_entry() {
        let mut app = app();
        app.handle_agent(AgentEvent::TurnStart { step: 1 });
        app.handle_agent(AgentEvent::TextDelta("hello".into()));
        app.handle_agent(AgentEvent::TurnEnd {
            stop_reason: StopReason::EndTurn,
        });
        assert!(matches!(
            app.transcript.last(),
            Some(Entry::Assistant(text)) if text == "hello"
        ));

        app.handle_agent(AgentEvent::Done {
            stop_reason: StopReason::EndTurn,
        });
        assert!(!app.busy);
    }

    #[test]
    fn tool_lifecycle_updates_entry() {
        let mut app = app();
        app.handle_agent(AgentEvent::ToolCall {
            id: "c1".into(),
            name: "read".into(),
            summary: "read a.txt".into(),
        });
        app.handle_agent(AgentEvent::ToolStart {
            id: "c1".into(),
            name: "read".into(),
        });
        app.handle_agent(AgentEvent::ToolEnd {
            id: "c1".into(),
            name: "read".into(),
            content: "data".into(),
            is_error: false,
        });

        match app.transcript.last() {
            Some(Entry::Tool {
                content,
                running,
                is_error,
                ..
            }) => {
                assert_eq!(content, "data");
                assert!(!running);
                assert!(!is_error);
            }
            _ => panic!("expected a tool entry"),
        }
    }

    #[test]
    fn permission_modal_answers_allow() {
        let mut app = app();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        app.set_permission(
            crate::permissions::PermissionRequest {
                tool: "bash".into(),
                summary: "run ls".into(),
                detail: "ls".into(),
                always_rule: None,
            },
            tx,
        );

        let consumed = app.handle_permission_key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        ));
        assert!(consumed);
        assert_eq!(rx.try_recv().unwrap(), Decision::AllowOnce);
        assert!(app.permission.is_none());
    }

    #[test]
    fn permission_modal_denies_on_escape() {
        let mut app = app();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        app.set_permission(
            crate::permissions::PermissionRequest {
                tool: "bash".into(),
                summary: "run rm".into(),
                detail: "rm x".into(),
                always_rule: None,
            },
            tx,
        );

        app.handle_permission_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(rx.try_recv().unwrap(), Decision::Deny);
    }

    #[test]
    fn retry_event_shows_transient_info() {
        let mut app = app();
        app.handle_agent(AgentEvent::Retry {
            attempt: 1,
            max_retries: 2,
            delay_ms: 500,
            error: "HTTP 429: slow down".into(),
        });

        let (text, _) = app.info.clone().expect("retry should set the status message");
        assert!(text.contains("retrying 1/2"), "got {text}");
        assert!(text.contains("HTTP 429"), "got {text}");
        assert!(app.transcript.is_empty(), "retries must not clutter the transcript");
    }

    #[test]
    fn done_event_clears_busy() {
        let mut app = app();
        app.busy = true;
        app.handle_agent(AgentEvent::Done {
            stop_reason: StopReason::EndTurn,
        });
        assert!(!app.busy);
    }

    #[test]
    fn compact_events_clear_busy_and_add_boundary() {
        let mut app = app();
        app.busy = true;

        app.handle_agent(AgentEvent::CompactStart {
            messages: 6,
            tokens: 120,
        });
        let (info, _) = app.info.clone().expect("info should be set");
        assert!(info.contains("compacting 6 messages"), "got {info}");

        app.handle_agent(AgentEvent::CompactEnd(CompactOutcome::Compacted(
            CompactReport {
                messages_before: 6,
                messages_after: 1,
                tokens_before: 120,
                tokens_after: 15,
                summary_chars: 60,
                truncated: false,
            },
        )));
        assert!(!app.busy);

        match app.transcript.last() {
            Some(Entry::Info(text)) => {
                assert!(text.contains("compacted 6 messages"), "got {text}");
                assert!(text.contains("~120 → ~15 tokens"), "got {text}");
            }
            _ => panic!("expected compact boundary info"),
        }
    }

    #[test]
    fn compact_empty_and_cancelled_clear_busy() {
        let mut app = app();
        app.busy = true;
        app.handle_agent(AgentEvent::CompactEnd(CompactOutcome::Empty));
        assert!(!app.busy);
        assert!(app.info.as_ref().unwrap().0.contains("nothing to compact"));

        app.busy = true;
        app.handle_agent(AgentEvent::CompactEnd(CompactOutcome::Cancelled));
        assert!(!app.busy);
        assert!(app.info.as_ref().unwrap().0.contains("cancelled"));
    }
}
