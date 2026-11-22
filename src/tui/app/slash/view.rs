use super::super::{App, Ctx, Entry};
use crate::config::Config;
use crate::memory::MemoryStore;
use crate::tools::{ExecutionMode, resolve_path};

impl App {
    pub(super) fn help_command(&mut self) {
        self.transcript
            .push(Entry::Info(crate::tui::commands::help_text()));
    }

    pub(super) fn status_command(&mut self, ctx: &Ctx) {
        self.transcript.push(Entry::Info(format!(
            "version: {}\nprovider: {}\nmodel: {}\npermission: {}\nsession: {}\ntokens: ↑{} ↓{}\ntools: {} · skills: {} · agents: {} · mcp: {}",
            crate::version::SHORT,
            self.status.provider,
            self.status.model,
            ctx.permissions.mode().label(),
            self.status.session,
            self.status.input_tokens,
            self.status.output_tokens,
            ctx.tools.names().len(),
            ctx.skills.len(),
            ctx.agents.agents.len(),
            ctx.mcp.tools().len()
        )));
    }

    pub(super) fn version_command(&mut self) {
        self.transcript
            .push(Entry::Info(crate::version::long()));
    }

    pub(super) fn context_command(&mut self, ctx: &Ctx) {
        let mut lines: Vec<String> = Vec::new();
        match ctx.agent.try_lock() {
            Ok(agent) => {
                let system: usize = agent.system().iter().map(String::len).sum();
                let messages: usize = agent
                    .messages()
                    .iter()
                    .map(|message| message.text().chars().count())
                    .sum();
                lines.push(format!(
                    "system prompt: {system} chars (~{} tokens)",
                    system / 4
                ));
                lines.push(format!(
                    "conversation: {} messages, {messages} chars (~{} tokens)",
                    agent.messages().len(),
                    messages / 4
                ));
            }
            Err(_) => lines.push("conversation: busy — try again after the turn".into()),
        }
        lines.push(format!(
            "transcript: {} entries · todos: {} · team tasks: {}",
            self.transcript.len(),
            self.todos.len(),
            self.team
                .as_ref()
                .map(|snapshot| snapshot.tasks.len())
                .unwrap_or(0)
        ));
        lines.push(format!(
            "tools: {} · skills: {} · agents: {} · mcp tools: {}",
            ctx.tools.names().len(),
            ctx.skills.len(),
            ctx.agents.agents.len(),
            ctx.mcp.tools().len()
        ));
        lines.push(format!(
            "tokens: ↑{} ↓{} · session: {}",
            self.status.input_tokens, self.status.output_tokens, self.status.session
        ));
        self.transcript.push(Entry::Info(lines.join("\n")));
    }

    pub(super) fn tools_command(&mut self, ctx: &Ctx) {
        let mut text = format!("tools ({}):\n", ctx.tools.names().len());
        for tool in ctx.tools.all() {
            let mode = match tool.execution_mode() {
                ExecutionMode::Parallel => "parallel",
                ExecutionMode::Sequential => "serial",
            };
            text.push_str(&format!(
                "  {:<18} {:<9} {}\n",
                tool.name(),
                mode,
                truncate(tool.description(), 72)
            ));
        }
        text.push_str("\nmcp tools are named mcp__<server>__<tool> · change access with /permissions");
        self.transcript.push(Entry::Info(text));
    }

    pub(super) fn export_command(&mut self, args: &str) {
        let path = if args.is_empty() {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            self.cwd.join(format!("mybot-transcript-{stamp}.md"))
        } else {
            resolve_path(&self.cwd, args)
        };
        let markdown = export_markdown(self);
        match std::fs::write(&path, markdown) {
            Ok(()) => self.info(format!("exported transcript to {}", path.display())),
            Err(error) => self.info(format!("cannot write {}: {error}", path.display())),
        }
    }

    pub(super) fn config_command(&mut self) {
        let path = crate::config::config_path();
        match Config::load() {
            Ok(config) => {
                let mut text = format!("config: {}\n", path.display());
                text.push_str(&format!(
                    "default provider: {}\npermission mode: {} · persisted rules: {}\n\nproviders:\n",
                    config.default_provider.as_deref().unwrap_or("(auto)"),
                    config.permissions.mode.label(),
                    config.permissions.rules.len()
                ));
                for (name, provider) in &config.providers {
                    let model = provider
                        .model
                        .clone()
                        .unwrap_or_else(|| provider.kind.default_model().to_string());
                    let base = provider
                        .base_url
                        .clone()
                        .unwrap_or_else(|| provider.kind.default_base_url().to_string());
                    let ready = config.resolve(Some(&format!("{name}/{model}"))).is_ok();
                    text.push_str(&format!(
                        "  {:<12} {:<28} {:<40} {}\n",
                        name,
                        model,
                        base,
                        if ready { "ready" } else { "needs key" }
                    ));
                }
                text.push_str(&format!(
                    "\nsessions: {}\nmemory: {}\nconfig file: {}",
                    crate::config::sessions_dir().display(),
                    MemoryStore::load(&self.cwd).root.display(),
                    path.display()
                ));
                self.transcript.push(Entry::Info(text));
            }
            Err(error) => self.info(format!("cannot read config: {error}")),
        }
    }

    pub(super) fn doctor_command(&mut self, ctx: &Ctx) {
        let mut lines = vec![format!("mybot {}", crate::version::long())];
        lines.push(format!("cwd: {}", self.cwd.display()));
        let config_path = crate::config::config_path();
        lines.push(format!(
            "config: {} ({})",
            config_path.display(),
            if config_path.exists() { "found" } else { "missing, using defaults" }
        ));

        match Config::load() {
            Ok(config) => {
                let ready = config
                    .providers
                    .iter()
                    .filter(|(name, provider)| {
                        let model = provider
                            .model
                            .clone()
                            .unwrap_or_else(|| provider.kind.default_model().to_string());
                        config.resolve(Some(&format!("{name}/{model}"))).is_ok()
                    })
                    .count();
                lines.push(format!(
                    "providers: {} configured, {} ready",
                    config.providers.len(),
                    ready
                ));
                match config.resolve(None) {
                    Ok(resolved) => {
                        lines.push(format!("active: {}/{}", resolved.name, resolved.model))
                    }
                    Err(error) => lines.push(format!("active: not ready — {error}")),
                }
            }
            Err(error) => lines.push(format!("config error: {error}")),
        }

        let sessions = crate::config::sessions_dir();
        let writable = std::fs::create_dir_all(&sessions).is_ok();
        lines.push(format!(
            "sessions: {} ({})",
            sessions.display(),
            if writable { "writable" } else { "not writable" }
        ));
        lines.push(format!(
            "memory: {}",
            MemoryStore::load(&self.cwd).root.display()
        ));
        lines.push(format!(
            "tools: {} · skills: {} · agents: {} · mcp tools: {}",
            ctx.tools.names().len(),
            ctx.skills.len(),
            ctx.agents.agents.len(),
            ctx.mcp.tools().len()
        ));
        if ctx.mcp.warnings().is_empty() {
            lines.push("mcp: no warnings".into());
        } else {
            for warning in ctx.mcp.warnings() {
                lines.push(format!("mcp warning: {warning}"));
            }
        }
        lines.push(format!(
            "session: {} · permission: {}",
            self.status.session,
            ctx.permissions.mode().label()
        ));
        self.transcript.push(Entry::Info(lines.join("\n")));
    }

    pub(super) fn keys_command(&mut self) {
        self.transcript.push(Entry::Info(
            "keys:\n\
             Enter              send\n\
             Shift+Enter        newline\n\
             Up/Down            history when the composer is empty\n\
             Esc                cancel the turn / clear the composer\n\
             Ctrl+C             cancel the turn, or exit when idle\n\
             Ctrl+D             exit when the composer is empty\n\
             PgUp/PgDn          scroll the transcript\n\
             /                  open the command palette\n\
             /model             searchable model picker"
                .into(),
        ));
    }

    pub(super) fn skills_command(&mut self, ctx: &Ctx) {
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
            text.push_str("load one with the `skill` tool, or /skill <name>\n");
        }
        if !ctx.plugins.commands.is_empty() {
            text.push_str(&format!(
                "\nplugin commands ({}):\n",
                ctx.plugins.commands.len()
            ));
            for command in &ctx.plugins.commands {
                text.push_str(&format!(
                    "  /{:<16} {:<12} {}\n",
                    command.name, command.plugin, command.description
                ));
            }
        }
        self.transcript
            .push(Entry::Info(text.trim_end().to_string()));
    }

    pub(super) fn agents_command(&mut self, ctx: &Ctx) {
        if ctx.agents.is_empty() {
            self.info("no agent definitions; add .mybot/agents/<name>.md");
        } else {
            let mut text = format!("agent types ({}):\n", ctx.agents.agents.len());
            for agent in &ctx.agents.agents {
                let tools = agent
                    .tools
                    .as_ref()
                    .map(|tools| tools.join(", "))
                    .unwrap_or_else(|| "all".into());
                text.push_str(&format!(
                    "  {:<16} {:<30} tools: {tools}\n",
                    agent.name, agent.description
                ));
            }
            text.push_str("the model delegates with the `agent` tool");
            self.transcript.push(Entry::Info(text));
        }
    }

    pub(super) fn mcp_command(&mut self, ctx: &Ctx) {
        self.transcript.push(Entry::Info(ctx.mcp.describe()));
    }

    pub(super) fn team_command(&mut self) {
        match &self.team {
            Some(snapshot) => self
                .transcript
                .push(Entry::Info(crate::team::render_snapshot(snapshot))),
            None => self.info("no teammates or tasks yet — the model starts them with spawn_teammate"),
        }
    }
}

fn export_markdown(app: &App) -> String {
    let mut out = format!(
        "# mybot transcript\n\n- version: {}\n- provider/model: {}/{}\n- session: {}\n- exported: {}\n\n",
        crate::version::SHORT,
        app.status.provider,
        app.status.model,
        app.status.session,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    for entry in &app.transcript {
        match entry {
            Entry::User(text) => out.push_str(&format!("## User\n\n{text}\n\n")),
            Entry::Assistant(text) => out.push_str(&format!("## Assistant\n\n{text}\n\n")),
            Entry::Tool {
                name,
                summary,
                content,
                is_error,
                ..
            } => {
                out.push_str(&format!("### Tool `{name}` — {summary}\n\n"));
                if *is_error {
                    out.push_str("**error**\n\n");
                }
                out.push_str(&format!("```\n{}\n```\n\n", truncate(content, 16_000)));
            }
            Entry::Info(text) => out.push_str(&format!("> {text}\n\n")),
            Entry::Error(text) => out.push_str(&format!("> **error:** {text}\n\n")),
        }
    }
    out
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
