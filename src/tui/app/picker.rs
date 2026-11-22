use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::agent::ProviderSwap;
use crate::config::Config;
use crate::providers;

use super::{App, Ctx};

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

impl App {
    pub(super) fn handle_model_key(&mut self, key: KeyEvent, ctx: &Ctx) -> bool {
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
                self.transcript.push(super::Entry::Info(format!(
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
}
