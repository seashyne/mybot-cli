use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::CUSTOM_ENTRY_LABEL;
use crate::config::Config;
use crate::tui::app::{App, Ctx};
use crate::tui::theme;

#[derive(Clone, Debug)]
pub struct CustomForm {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// 0 = Base URL, 1 = Model ID, 2 = API Key
    pub field: usize,
}

impl CustomForm {
    pub fn for_provider(provider: &str, model: &str) -> Self {
        let cfg = Config::load_raw().unwrap_or_default();
        let existing = cfg.providers.get(provider);
        let preset = crate::providers::presets::find(provider);
        let base_url = existing
            .and_then(|p| p.base_url.clone())
            .or_else(|| preset.and_then(|p| p.base_url.map(str::to_string)))
            .unwrap_or_else(|| match provider {
                "anthropic" => "https://api.anthropic.com".to_string(),
                "openai" => "https://api.openai.com/v1".to_string(),
                _ => String::new(),
            });
        let resolved_model = if model == CUSTOM_ENTRY_LABEL || model.is_empty() {
            existing.and_then(|p| p.model.clone()).unwrap_or_default()
        } else {
            model.to_string()
        };
        let api_key = existing.and_then(|p| p.api_key.clone()).unwrap_or_default();
        let field = if base_url.is_empty() {
            0
        } else if resolved_model.is_empty() {
            1
        } else {
            2
        };
        Self {
            provider: provider.to_string(),
            base_url,
            model: resolved_model,
            api_key,
            field,
        }
    }

    pub fn active_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.base_url,
            1 => &mut self.model,
            _ => &mut self.api_key,
        }
    }

    pub fn render_lines(&self, height: u16, error: Option<&str>) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("configure ", Style::default().fg(theme::DIM)),
                Span::styled(
                    self.provider.clone(),
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  (custom URL / model / API key)",
                    Style::default().fg(theme::DIM),
                ),
            ]),
            Line::from(""),
        ];
        for (idx, (label, val, hint)) in [
            ("1. Base URL", self.base_url.as_str(), "https://api.example.com/v1"),
            ("2. Model ID", self.model.as_str(), "e.g. gpt-4o, auto, my-model"),
            ("3. API Key ", self.api_key.as_str(), "paste key (optional for localhost)"),
        ]
        .into_iter()
        .enumerate()
        {
            let active = self.field == idx;
            let marker = if active { "▸ " } else { "  " };
            let lbl_style = if active {
                Style::default()
                    .fg(theme::USER)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::DIM)
            };
            let display = if val.is_empty() && !active {
                Span::styled(hint, Style::default().fg(theme::DIM))
            } else {
                Span::styled(val.to_string(), Style::default().fg(theme::ASSISTANT))
            };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(theme::ACCENT)),
                Span::styled(format!("{label} : "), lbl_style),
                display,
                Span::styled(
                    if active { "█" } else { "" },
                    Style::default().fg(theme::ACCENT),
                ),
            ]));
            lines.push(Line::from(""));
        }
        while lines.len() < height.saturating_sub(1) as usize {
            lines.push(Line::from(""));
        }
        let footer = if let Some(err) = error {
            Line::from(Span::styled(err.to_string(), Style::default().fg(theme::ERROR)))
        } else {
            Line::from(Span::styled(
                "Enter next/save · ↑↓/Tab move · Ctrl+U clear field · Esc back",
                Style::default().fg(theme::DIM),
            ))
        };
        lines.push(footer);
        lines
    }
}

impl App {
    pub(super) fn handle_custom_form_key(
        &mut self,
        key: KeyEvent,
        control: bool,
        ctx: &Ctx,
    ) -> bool {
        let mut submit: Option<CustomForm> = None;
        if let Some(picker) = &mut self.model_picker
            && let Some(form) = &mut picker.custom_form
        {
            match key.code {
                KeyCode::Esc => {
                    picker.custom_form = None;
                    picker.error = None;
                    return true;
                }
                KeyCode::Char('c') if control => {
                    self.model_picker = None;
                    return true;
                }
                KeyCode::Char('u') | KeyCode::Char('w') if control => {
                    form.active_mut().clear();
                    picker.error = None;
                }
                KeyCode::Up | KeyCode::BackTab => {
                    form.field = (form.field + 2) % 3;
                    picker.error = None;
                }
                KeyCode::Down | KeyCode::Tab => {
                    form.field = (form.field + 1) % 3;
                    picker.error = None;
                }
                KeyCode::Backspace => {
                    form.active_mut().pop();
                    picker.error = None;
                }
                KeyCode::Char(ch) if !control => {
                    form.active_mut().push(ch);
                    picker.error = None;
                }
                KeyCode::Enter => {
                    if form.field == 0 {
                        let url = form.base_url.trim();
                        if !(url.starts_with("http://") || url.starts_with("https://")) {
                            picker.error =
                                Some("Base URL must start with http:// or https://".into());
                        } else {
                            form.field = 1;
                            picker.error = None;
                        }
                    } else if form.field == 1 {
                        if form.model.trim().is_empty() {
                            picker.error = Some("Model ID must not be empty".into());
                        } else {
                            form.field = 2;
                            picker.error = None;
                        }
                    } else {
                        let url = form.base_url.trim();
                        if !(url.starts_with("http://") || url.starts_with("https://")) {
                            form.field = 0;
                            picker.error =
                                Some("Base URL must start with http:// or https://".into());
                        } else if form.model.trim().is_empty() {
                            form.field = 1;
                            picker.error = Some("Model ID must not be empty".into());
                        } else {
                            submit = Some(form.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(form) = submit {
            let url = form.base_url.trim();
            let model = form.model.trim();
            let key = form.api_key.trim();
            if let Ok(mut config) = Config::load_raw() {
                if !config.providers.contains_key(&form.provider) {
                    let default_cfg = crate::providers::presets::find(&form.provider)
                        .map(|p| p.to_config(None, None, model))
                        .unwrap_or_else(|| {
                            crate::config::ProviderConfig::defaults_for(
                                crate::config::ProviderKind::OpenAi,
                            )
                        });
                    config.providers.insert(form.provider.clone(), default_cfg);
                }
                if let Some(entry) = config.providers.get_mut(&form.provider) {
                    entry.base_url = Some(url.to_string());
                    entry.model = Some(model.to_string());
                    if !key.is_empty() {
                        entry.api_key = Some(key.to_string());
                    }
                }
                let _ = config.save();
            }
            self.apply_model(ctx, &form.provider, model);
        }
        true
    }
}
