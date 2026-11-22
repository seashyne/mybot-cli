//! First-run setup: pick a provider, optionally paste a key, choose a model.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::ProviderConfig;
use crate::providers::presets::{self, Preset};

mod screen;
#[cfg(test)]
mod tests;

pub use screen::run;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Provider,
    BaseUrl,
    Key,
    Model,
}

struct State {
    step: Step,
    choice: usize,
    scroll: usize,
    base_url: String,
    key: String,
    model: String,
    error: Option<String>,
}

enum Action {
    Continue,
    Cancel,
    Done(Box<ProviderConfig>, &'static str),
}

impl State {
    fn new() -> Self {
        Self {
            step: Step::Provider,
            choice: 0,
            scroll: 0,
            base_url: String::new(),
            key: String::new(),
            model: String::new(),
            error: None,
        }
    }

    fn choice(&self) -> &'static Preset {
        &presets::PRESETS[self.choice.min(presets::PRESETS.len() - 1)]
    }

    fn steps(&self) -> Vec<Step> {
        let preset = self.choice();
        let mut steps = vec![Step::Provider];
        if preset.name == "custom" {
            steps.push(Step::BaseUrl);
        }
        if preset.needs_key || preset.name == "custom" {
            steps.push(Step::Key);
        }
        steps.push(Step::Model);
        steps
    }

    fn step_index(&self) -> usize {
        self.steps()
            .iter()
            .position(|step| *step == self.step)
            .unwrap_or(0)
    }

    fn advance(&mut self) -> Action {
        match self.step {
            Step::Provider => {
                let preset = self.choice();
                self.base_url = preset.base_url.unwrap_or("").to_string();
                self.model = preset.model.to_string();
                self.step = self.steps().get(1).copied().unwrap_or(Step::Model);
                self.error = None;
                Action::Continue
            }
            Step::BaseUrl => {
                let url = self.base_url.trim();
                if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
                    self.error = Some("base URL must start with http:// or https://".into());
                    return Action::Continue;
                }
                let next = self.step_index() + 1;
                self.step = self.steps().get(next).copied().unwrap_or(Step::Model);
                self.error = None;
                Action::Continue
            }
            Step::Key => {
                let trimmed = self.key.trim();
                if trimmed.is_empty() && self.choice().needs_key {
                    self.error = Some("an API key is required for this provider".into());
                    return Action::Continue;
                }
                if !trimmed.is_empty() && trimmed.len() < 8 {
                    self.error = Some("that key looks too short".into());
                    return Action::Continue;
                }
                self.step = Step::Model;
                self.error = None;
                Action::Continue
            }
            Step::Model => {
                let preset = self.choice();
                let model = if self.model.trim().is_empty() {
                    if preset.name == "custom" {
                        self.error = Some("enter a model name".into());
                        return Action::Continue;
                    }
                    preset.model.to_string()
                } else {
                    self.model.trim().to_string()
                };
                Action::Done(Box::new(self.build(&model)), preset.name)
            }
        }
    }

    fn back(&mut self) -> Action {
        let steps = self.steps();
        let index = self.step_index();
        if index == 0 {
            return Action::Cancel;
        }
        self.step = steps[index - 1];
        self.error = None;
        Action::Continue
    }

    fn build(&self, model: &str) -> ProviderConfig {
        let base_url = if self.base_url.trim().is_empty() {
            None
        } else {
            Some(self.base_url.trim().to_string())
        };
        let key = if self.key.trim().is_empty() {
            None
        } else {
            Some(self.key.trim().to_string())
        };
        self.choice().to_config(base_url, key, model)
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if control => return Action::Cancel,
            KeyCode::Esc => return self.back(),
            KeyCode::Up | KeyCode::Char('k') if self.step == Step::Provider => {
                self.choice = self.choice.saturating_sub(1);
                self.error = None;
                return Action::Continue;
            }
            KeyCode::Down | KeyCode::Char('j') if self.step == Step::Provider => {
                self.choice = (self.choice + 1).min(presets::PRESETS.len() - 1);
                self.error = None;
                return Action::Continue;
            }
            KeyCode::PageUp if self.step == Step::Provider => {
                self.choice = self.choice.saturating_sub(6);
                return Action::Continue;
            }
            KeyCode::PageDown if self.step == Step::Provider => {
                self.choice = (self.choice + 6).min(presets::PRESETS.len() - 1);
                return Action::Continue;
            }
            KeyCode::Enter => return self.advance(),
            KeyCode::Backspace => {
                if let Some(buffer) = self.buffer() {
                    buffer.pop();
                }
                return Action::Continue;
            }
            KeyCode::Char(ch) if !control => {
                if let Some(buffer) = self.buffer() {
                    buffer.push(ch);
                }
                return Action::Continue;
            }
            _ => {}
        }
        Action::Continue
    }

    fn buffer(&mut self) -> Option<&mut String> {
        match self.step {
            Step::Provider => None,
            Step::BaseUrl => Some(&mut self.base_url),
            Step::Key => Some(&mut self.key),
            Step::Model => Some(&mut self.model),
        }
    }
}
