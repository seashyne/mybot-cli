use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::agent::ProviderSwap;
use crate::config::Config;
use crate::providers;
use crate::providers::catalog;

use super::{App, Ctx};

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
        Self {
            all,
            query: query.to_string(),
            selected: 0,
            error: None,
            loading: false,
            pending: 0,
            notes: Vec::new(),
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
                query.is_empty()
                    || format!("{}/{}", item.provider, item.model)
                        .to_ascii_lowercase()
                        .contains(&query)
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

        enum Act {
            None,
            Close,
            ClearQuery,
            Move(isize),
            Edit(Option<char>),
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
                        Act::Apply(item.provider.clone(), item.model.clone())
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
            Act::Apply(provider, model) => self.apply_model(ctx, &provider, &model),
        }
        true
    }

    pub(super) fn apply_model(&mut self, ctx: &Ctx, provider: &str, model: &str) {
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
                self.transcript.push(super::Entry::Info(format!(
                    "switched to {provider}/{model} (saved to config)"
                )));
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
mod tests {
    use super::*;
    use crate::config::{Config, ProviderConfig};

    fn config() -> Config {
        let mut config = Config::default();
        config.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                model: Some("deepseek-reasoner".into()),
                ..ProviderConfig::defaults_for(crate::config::ProviderKind::OpenAi)
            },
        );
        config
    }

    #[test]
    fn lists_every_catalog_model_per_provider() {
        let picker = ModelPicker::from_config(&config());
        let anthropic: Vec<&ModelItem> = picker
            .all
            .iter()
            .filter(|item| item.provider == "anthropic")
            .collect();
        assert!(anthropic.len() >= 3);
    }

    #[test]
    fn configured_model_is_first() {
        let picker = ModelPicker::from_config(&config());
        let deepseek: Vec<&ModelItem> = picker
            .all
            .iter()
            .filter(|item| item.provider == "deepseek")
            .collect();
        assert_eq!(deepseek[0].model, "deepseek-reasoner");
    }

    #[test]
    fn query_filters_by_provider_or_model() {
        let mut picker = ModelPicker::from_config(&config());
        picker.query = "sonnet".into();
        let visible = picker.visible();
        assert!(!visible.is_empty());
        assert!(visible.iter().all(|item| item.model.contains("sonnet")));

        picker.query = "deepseek/".into();
        let visible = picker.visible();
        assert!(!visible.is_empty());
        assert!(visible.iter().all(|item| item.provider == "deepseek"));
    }

    #[test]
    fn live_models_are_prepended_and_catalog_entries_kept() {
        let mut picker = ModelPicker::from_config(&config());
        picker.merge_models(
            "anthropic",
            &["claude-live".into(), "claude-sonnet-4-5".into()],
        );
        let anthropic: Vec<&ModelItem> = picker
            .all
            .iter()
            .filter(|item| item.provider == "anthropic")
            .collect();
        assert_eq!(anthropic[0].model, "claude-live");
        assert!(anthropic.iter().any(|item| item.model == "claude-opus-4-1"));
        assert_eq!(
            anthropic
                .iter()
                .filter(|item| item.model == "claude-sonnet-4-5")
                .count(),
            1
        );
    }

    #[test]
    fn selection_wraps() {
        let mut picker = ModelPicker::from_config(&config());
        let len = picker.visible().len();
        picker.selected = len - 1;
        picker.move_selection(1);
        assert_eq!(picker.selected, 0);
        picker.move_selection(-1);
        assert_eq!(picker.selected, len - 1);
    }
}
