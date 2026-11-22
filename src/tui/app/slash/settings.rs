use std::time::Duration;

use super::super::{App, Ctx, Entry, ModelPicker, UiEvent};
use crate::config::{Config, EffortLevel, PermissionMode};
use crate::memory::{MemoryKind, MemoryStore};

impl App {
    pub(super) fn model_command(&mut self, args: &str, ctx: &Ctx) {
        let config = match Config::load() {
            Ok(config) => config,
            Err(error) => {
                self.info(format!("cannot read config: {error}"));
                return;
            }
        };
        let picker = ModelPicker::from_config(&config);
        if picker.all.is_empty() {
            self.info("no providers configured");
            return;
        }

        if let Some(item) = picker
            .all
            .iter()
            .find(|item| format!("{}/{}", item.provider, item.model) == args)
        {
            let (provider, model) = (item.provider.clone(), item.model.clone());
            self.apply_model(ctx, &provider, &model);
            return;
        }
        if let Some((provider, model)) = args.split_once('/')
            && config.providers.contains_key(provider)
            && !model.trim().is_empty()
        {
            self.apply_model(ctx, provider, model.trim());
            return;
        }

        let mut picker = if args.is_empty() {
            picker
        } else {
            ModelPicker::with_query(&config, args)
        };
        picker.select_current(&self.status.provider, &self.status.model);

        let mut pending = 0usize;
        for (name, provider) in &config.providers {
            let model = provider
                .model
                .clone()
                .unwrap_or_else(|| provider.kind.default_model().to_string());
            let Ok(resolved) = config.resolve(Some(&format!("{name}/{model}"))) else {
                continue;
            };
            pending += 1;
            let tx = ctx.ui_tx.clone();
            let name = name.clone();
            tokio::spawn(async move {
                let timeout = if crate::config::is_local_url(&resolved.base_url) {
                    Duration::from_secs(4)
                } else {
                    Duration::from_secs(10)
                };
                let (models, error) =
                    match tokio::time::timeout(timeout, crate::providers::list_models(&resolved))
                        .await
                    {
                        Ok(Ok(models)) => (models, None),
                        Ok(Err(error)) => (Vec::new(), Some(error.to_string())),
                        Err(_) => (
                            Vec::new(),
                            Some(format!("timed out after {}s", timeout.as_secs())),
                        ),
                    };
                let _ = tx.send(UiEvent::Models {
                    provider: name,
                    models,
                    error,
                });
            });
        }
        picker.pending = pending;
        picker.loading = pending > 0;
        self.model_picker = Some(picker);
    }

    pub(super) fn permissions_command(&mut self, args: &str, ctx: &Ctx) {
        if args.is_empty() {
            let mut text = format!("permission mode: {}\n", ctx.permissions.mode().label());
            text.push_str(
                "  ask   confirm writes and commands\n\
                 auto  allow safe reads, ask for the rest\n\
                 full  never prompt\n",
            );
            text.push_str(&format!(
                "persisted rules: {}\nchange with /permissions ask|auto|full",
                ctx.permissions.rule_count()
            ));
            self.transcript.push(Entry::Info(text));
            return;
        }

        let Some(mode) = PermissionMode::parse(args) else {
            self.info("usage: /permissions [ask|auto|full]");
            return;
        };
        ctx.permissions.set_mode(mode);
        self.status.permission = mode;
        let saved = Config::load().and_then(|mut config| {
            config.permissions.mode = mode;
            config.save()
        });
        match saved {
            Ok(()) => self.info(format!("permission mode: {} (saved)", mode.label())),
            Err(error) => self.info(format!(
                "permission mode: {} (not saved: {error})",
                mode.label()
            )),
        }
    }

    pub(super) fn effort_command(&mut self, args: &str, ctx: &Ctx) {
        if args.is_empty() {
            let current = EffortLevel::from_budget(self.status.thinking_budget);
            let mut text = format!("reasoning effort: {}\n", current.label());
            text.push_str(
                "  off     no extended thinking budget\n\
                 low     light reasoning (4k budget)\n\
                 medium  balanced reasoning (16k budget)\n\
                 high    deep reasoning (32k budget)\n\
                 change with /effort off|low|medium|high",
            );
            self.transcript.push(Entry::Info(text));
            return;
        }

        let Some(level) = EffortLevel::parse(args) else {
            self.info("usage: /effort [off|low|medium|high]");
            return;
        };
        let budget = level.budget();
        self.status.thinking_budget = budget;
        let agent = ctx.agent.clone();
        tokio::spawn(async move {
            agent.lock().await.set_thinking_budget(budget);
        });

        let provider_name = self.status.provider.clone();
        let saved = Config::load().and_then(|mut config| {
            if let Some(entry) = config.providers.get_mut(&provider_name) {
                entry.thinking_budget = budget;
                config.save()?;
            }
            Ok(())
        });
        match saved {
            Ok(()) => self.info(format!("effort: {} (saved)", level.label())),
            Err(error) => self.info(format!("effort: {} (not saved: {error})", level.label())),
        }
    }

    pub(super) fn skill_command(&mut self, args: &str, ctx: &Ctx) {
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

    pub(super) fn memory_command(&mut self, args: &str) {
        let mut store = MemoryStore::load(&self.cwd);
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

    pub(super) fn remember_command(&mut self, args: &str) {
        if args.is_empty() {
            self.info("usage: /remember <text>");
            return;
        }
        let mut store = MemoryStore::load(&self.cwd);
        let title: String = args
            .lines()
            .next()
            .unwrap_or(args)
            .chars()
            .take(60)
            .collect();
        let name = format!("note-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        match store.write(&name, &title, &title, MemoryKind::Project, args) {
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

    pub(super) fn init_command(&mut self) {
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

    pub(super) fn plugin_command(&mut self, name: &str, args: &str, ctx: &Ctx) {
        let command_name = name.trim_start_matches('/');
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
            self.info(format!("unknown command /{command_name} (type / for the palette)"));
        }
    }
}
