use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::config::Config;

use super::{App, Ctx, Entry, ModelPicker, short_id};

impl App {
    pub(super) fn slash(&mut self, command: &str, ctx: &Ctx) {
        let (name, args) = command.split_once(' ').unwrap_or((command, ""));
        let args = args.trim();

        match name {
            "/quit" | "/exit" => self.quit = true,
            "/help" => {
                self.transcript.push(Entry::Info(
                    "commands: /help /new /model /skills /skill /agents /sessions /status /version /compact /memory /remember /mcp /team /init /quit\n\
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
                    "version: {}\nprovider: {}\nmodel: {}\npermission: {}\nsession: {}\ntokens: ↑{} ↓{}",
                    crate::version::SHORT,
                    self.status.provider,
                    self.status.model,
                    self.status.permission.label(),
                    self.status.session,
                    self.status.input_tokens,
                    self.status.output_tokens
                )));
            }
            "/version" => self
                .transcript
                .push(Entry::Info(crate::version::long())),
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
                self.transcript.push(Entry::Info(text.trim_end().to_string()));
            }
            "/agents" => {
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
            "/memory" => self.memory_command(args),
            "/mcp" => {
                self.transcript
                    .push(Entry::Info(ctx.mcp.describe()));
            }
            "/team" => match &self.team {
                Some(snapshot) => self
                    .transcript
                    .push(Entry::Info(crate::team::render_snapshot(snapshot))),
                None => self.info("no teammates or tasks yet — the model starts them with spawn_teammate"),
            },
            "/remember" => {
                if args.is_empty() {
                    self.info("usage: /remember <text>");
                    return;
                }
                let mut store = crate::memory::MemoryStore::load(&self.cwd);
                let title: String = args
                    .lines()
                    .next()
                    .unwrap_or(args)
                    .chars()
                    .take(60)
                    .collect();
                let name = format!("note-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
                match store.write(
                    &name,
                    &title,
                    &title,
                    crate::memory::MemoryKind::Project,
                    args,
                ) {
                    Ok(path) => {
                        self.info(format!("saved memory to {}", path.display()));
                        self.transcript.push(Entry::Info(format!(
                            "memory saved: {}",
                            crate::memory::slugify(&name)
                        )));
                    }
                    Err(error) => self.info(format!("cannot save memory: {error}")),
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
                    let template =
                        "# Project instructions\n\n- Build:\n- Test:\n- Conventions:\n";
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

    fn memory_command(&mut self, args: &str) {
        let mut store = crate::memory::MemoryStore::load(&self.cwd);
        let args = args.trim();

        if args.is_empty() {
            let mut text = store.render_index();
            text.push_str(&format!(
                "\nroot: {}\n/remember <text> saves a note · /memory delete <name> removes one",
                store.root.display()
            ));
            self.transcript.push(Entry::Info(text));
            return;
        }

        if let Some(name) = args
            .strip_prefix("delete ")
            .or_else(|| args.strip_prefix("forget "))
        {
            let name = name.trim();
            match store.delete(name) {
                Ok(true) => self.info(format!(
                    "deleted memory '{}'",
                    crate::memory::slugify(name)
                )),
                Ok(false) => self.info(format!("unknown memory '{name}'")),
                Err(error) => self.info(format!("cannot delete memory: {error}")),
            }
            return;
        }

        let name = args.strip_prefix("show ").unwrap_or(args).trim();
        match store.get(name) {
            Some(entry) => self.transcript.push(Entry::Info(format!(
                "# {} ({})\n{}\n\n{}",
                entry.title,
                entry.kind.as_str(),
                entry.description,
                entry.body
            ))),
            None => self.info(format!("unknown memory '{name}' (try /memory)")),
        }
    }
}
