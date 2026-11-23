use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::agent::ProviderSwap;
use crate::config::Config;
use crate::providers;
use crate::providers::catalog;

use super::{App, Ctx};

mod custom;

pub use custom::CustomForm;

pub const CUSTOM_ENTRY_LABEL: &str = "+ custom URL / model / key…";

pub struct ModelItem {
    pub provider: String,
    pub model: String,
    pub ready: bool,
}

pub struct ModelPicker {
    pub all: Vec<ModelItem>,
    pub query: String,
    pub selected: usize,
    pub error: Option<String>,
    /// Providers are still being queried for their live model list.
    pub loading: bool,
    pub pending: usize,
    pub notes: Vec<String>,
    pub custom_form: Option<CustomForm>,
}

impl ModelPicker {
    pub fn from_config(config: &Config) -> Self {
        Self::with_query(config, "")
    }

    pub fn with_query(config: &Config, query: &str) -> Self {
        let mut all = Vec::new();
        for (name, provider) in &config.providers {
            let configured = provider
                .model
                .clone()
                .unwrap_or_else(|| provider.kind.default_model().to_string());
            for model in catalog::with_configured(name, Some(&configured)) {
                let ready = config
                    .resolve(Some(&format!("{name}/{model}")))
                    .is_ok();
                all.push(ModelItem {
                    provider: name.clone(),
                    model,
                    ready,
                });
            }
        }
        all.push(ModelItem {
            provider: "custom".to_string(),
            model: CUSTOM_ENTRY_LABEL.to_string(),
            ready: true,
        });
        for preset in crate::providers::presets::PRESETS {
            if preset.name == "custom" || config.providers.contains_key(preset.name) {
                continue;
            }
            let ready = !preset.needs_key
                || std::env::var(preset.key_env)
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false);
            for model in catalog::with_configured(preset.name, Some(preset.model)) {
                all.push(ModelItem {
                    provider: preset.name.to_string(),
                    model,
                    ready,
                });
            }
        }
        Self {
            all,
            query: query.to_string(),
            selected: 0,
            error: None,
            loading: false,
            pending: 0,
            notes: Vec::new(),
            custom_form: None,
        }
    }

    /// Replace a provider's rows with the live list from its API, keeping any
    /// configured or catalog models the API did not report.
    pub fn merge_models(&mut self, provider: &str, models: &[String]) {
        let selected = self
            .visible()
            .get(self.selected)
            .map(|item| (item.provider.clone(), item.model.clone()));
        let count = self
            .all
            .iter()
            .filter(|item| item.provider == provider)
            .count();
        let insert_at = self
            .all
            .iter()
            .position(|item| item.provider == provider)
            .unwrap_or(self.all.len());

        let mut replacement: Vec<ModelItem> = models
            .iter()
            .map(|model| ModelItem {
                provider: provider.to_string(),
                model: model.clone(),
                ready: true,
            })
            .collect();
        for item in self.all.iter().filter(|item| item.provider == provider) {
            if !replacement.iter().any(|new| new.model == item.model) {
                replacement.push(ModelItem {
                    provider: item.provider.clone(),
                    model: item.model.clone(),
                    ready: item.ready,
                });
            }
        }
        self.all.splice(insert_at..insert_at + count, replacement);

        if let Some((provider, model)) = selected {
            self.select_current(&provider, &model);
        }
        let len = self.visible().len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
    }

    /// Items matching the current search text, in catalog order.
    pub fn visible(&self) -> Vec<&ModelItem> {
        let query = self.query.trim().to_ascii_lowercase();
        self.all
            .iter()
            .filter(|item| {
                if query.is_empty() {
                    return true;
                }
                if let Some((prov, rest)) = query.split_once('/') {
                    item.provider.to_ascii_lowercase().contains(prov)
                        && item.model.to_ascii_lowercase().contains(rest)
                } else {
                    format!("{}/{}", item.provider, item.model)
                        .to_ascii_lowercase()
                        .contains(&query)
                }
            })
            .collect()
    }

    pub fn select_current(&mut self, provider: &str, model: &str) {
        let position = {
            let visible = self.visible();
            visible
                .iter()
                .position(|item| item.provider == provider && item.model == model)
        };
        if let Some(position) = position {
            self.selected = position;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.rem_euclid(len as isize) as usize;
        self.error = None;
    }

    fn reset_selection(&mut self) {
        self.selected = 0;
        self.error = None;
    }
}

impl App {
    pub(super) fn handle_model_key(&mut self, key: KeyEvent, ctx: &Ctx) -> bool {
        if self.model_picker.is_none() {
            return false;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);

        if self
            .model_picker
            .as_ref()
            .is_some_and(|p| p.custom_form.is_some())
        {
            return self.handle_custom_form_key(key, control, ctx);
        }

        enum Act {
            None,
            Close,
            ClearQuery,
            Move(isize),
            Edit(Option<char>),
            OpenForm(String, String),
            Apply(String, String),
        }

        let act = {
            let picker = self.model_picker.as_ref().unwrap();
            match key.code {
                KeyCode::Esc => {
                    if picker.query.is_empty() {
                        Act::Close
                    } else {
                        Act::ClearQuery
                    }
                }
                KeyCode::Char('c') if control => Act::Close,
                KeyCode::Up => Act::Move(-1),
                KeyCode::Down => Act::Move(1),
                KeyCode::PageUp => Act::Move(-8),
                KeyCode::PageDown => Act::Move(8),
                KeyCode::Backspace => Act::Edit(None),
                KeyCode::Char(ch) if !control => Act::Edit(Some(ch)),
                KeyCode::Enter => {
                    let visible = picker.visible();
                    if let Some(item) = visible.get(picker.selected) {
                        if item.model == CUSTOM_ENTRY_LABEL || !item.ready {
                            Act::OpenForm(item.provider.clone(), item.model.clone())
                        } else {
                            Act::Apply(item.provider.clone(), item.model.clone())
                        }
                    } else {
                        let query = picker.query.trim();
                        if query.is_empty() {
                            Act::None
                        } else if let Some((provider, model)) = query.split_once('/') {
                            Act::Apply(provider.to_string(), model.to_string())
                        } else {
                            Act::Apply(self.status.provider.clone(), query.to_string())
                        }
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
            Act::ClearQuery => {
                if let Some(picker) = &mut self.model_picker {
                    picker.query.clear();
                    picker.reset_selection();
                }
            }
            Act::Move(delta) => {
                if let Some(picker) = &mut self.model_picker {
                    picker.move_selection(delta);
                }
            }
            Act::Edit(Some(ch)) => {
                if let Some(picker) = &mut self.model_picker {
                    picker.query.push(ch);
                    picker.reset_selection();
                }
            }
            Act::Edit(None) => {
                if let Some(picker) = &mut self.model_picker {
                    picker.query.pop();
                    picker.reset_selection();
                }
            }
            Act::OpenForm(provider, model) => {
                if let Some(picker) = &mut self.model_picker {
                    picker.custom_form = Some(CustomForm::for_provider(&provider, &model));
                    picker.error = None;
                }
            }
            Act::Apply(provider, model) => self.apply_model(ctx, &provider, &model),
        }
        true
    }

    pub(super) fn apply_model(&mut self, ctx: &Ctx, provider: &str, model: &str) {
        let mut needs_key_prompt = false;
        let attempt = (|| -> crate::error::Result<Option<u32>> {
            let mut config = Config::load()?;
            if !config.providers.contains_key(provider)
                && let Some(preset) = crate::providers::presets::find(provider)
            {
                config.providers.insert(
                    provider.to_string(),
                    preset.to_config(None, None, model),
                );
            }
            let entry = config.providers.get_mut(provider).ok_or_else(|| {
                crate::error::AppError::config(format!("unknown provider '{provider}'"))
            })?;
            entry.model = Some(model.to_string());

            let resolved = match config.resolve(Some(&format!("{provider}/{model}"))) {
                Ok(resolved) => resolved,
                Err(err) => {
                    if err.to_string().contains("API key") {
                        let _ = config.save();
                        needs_key_prompt = true;
                    }
                    return Err(err);
                }
            };
            config.default_provider = Some(provider.to_string());
            let built: Arc<dyn crate::providers::Provider> =
                Arc::from(providers::build(&resolved)?);
            config.save()?;

            let thinking_budget = resolved.thinking_budget;
            let swap = ProviderSwap {
                provider: built,
                max_tokens: resolved.max_tokens,
                temperature: resolved.temperature,
                thinking_budget,
                retry: providers::RetryPolicy::from_resolved(&resolved),
            };
            let agent = ctx.agent.clone();
            tokio::spawn(async move {
                agent.lock().await.set_provider(swap);
            });
            Ok(thinking_budget)
        })();

        match attempt {
            Ok(thinking_budget) => {
                self.status.provider = provider.to_string();
                self.status.model = model.to_string();
                self.status.thinking_budget = thinking_budget;
                self.model_picker = None;
                self.transcript.push(super::Entry::Info(format!(
                    "switched to {provider}/{model} (saved to config)"
                )));
            }
            Err(_) if needs_key_prompt => {
                let form = CustomForm::for_provider(provider, model);
                if let Some(picker) = &mut self.model_picker {
                    picker.custom_form = Some(form);
                    picker.error = Some(format!(
                        "enter API key for {provider} (or edit Base URL / Model ID)"
                    ));
                } else {
                    self.composer.set_text(&format!("/key {provider} "));
                    self.info(format!("paste API key for {provider} and press Enter"));
                }
            }
            Err(error) => {
                if let Some(picker) = &mut self.model_picker {
                    picker.error = Some(error.to_string());
                } else {
                    self.info(format!("cannot switch model: {error}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
