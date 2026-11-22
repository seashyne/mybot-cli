//! First-run setup: pick a provider, optionally paste a key, choose a model.

use std::io::Stdout;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::config::{Config, ProviderConfig};
use crate::error::{AppError, Result};
use crate::providers::presets::{self, Preset};

use super::theme;

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

/// Run the setup screen. Returns false when the user chose to quit.
pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    input: &mut mpsc::UnboundedReceiver<Event>,
    config: &mut Config,
) -> Result<bool> {
    let mut state = State::new();

    loop {
        terminal
            .draw(|frame| draw(frame, &mut state))
            .map_err(|error| AppError::runtime(format!("cannot draw: {error}")))?;

        let Some(event) = input.recv().await else {
            return Ok(false);
        };
        let Event::Key(key) = event else {
            continue;
        };

        match state.handle_key(key) {
            Action::Continue => {}
            Action::Cancel => return Ok(false),
            Action::Done(provider, name) => {
                config.providers.insert(name.to_string(), *provider);
                config.default_provider = Some(name.to_string());
                config.save()?;
                return Ok(true);
            }
        }
    }
}

fn draw(frame: &mut ratatui::Frame, state: &mut State) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(ratatui::style::Color::Black)),
        area,
    );

    let width = area.width.saturating_sub(6).min(76);
    let height = 20.min(area.height.saturating_sub(2));
    let rect = centered(area, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(Span::styled(
            " mybot setup ",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(inner);

    let steps = state.steps();
    let mut header = vec![Line::from(Span::styled(
        format!("step {} of {}", state.step_index() + 1, steps.len()),
        Style::default().fg(theme::DIM),
    ))];
    header.push(Line::from(Span::styled(
        match state.step {
            Step::Provider => "Choose a provider".to_string(),
            Step::BaseUrl => "Base URL of your endpoint".to_string(),
            Step::Key => format!("Paste your {} API key", state.choice().label),
            Step::Model => format!("Model for {}", state.choice().label),
        },
        Style::default()
            .fg(theme::ASSISTANT)
            .add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(Paragraph::new(header), rows[0]);

    match state.step {
        Step::Provider => draw_choices(frame, state, rows[1]),
        Step::BaseUrl | Step::Key | Step::Model => draw_input(frame, state, rows[1]),
    }

    let mut footer = Vec::new();
    if let Some(error) = &state.error {
        footer.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(theme::ERROR),
        )));
    }
    footer.push(Line::from(Span::styled(
        match state.step {
            Step::Provider => "↑↓ move · Enter select · Esc quit",
            Step::Key => "Enter confirm · Esc back · the key is saved to config.toml",
            _ => "Enter confirm · Esc back",
        },
        Style::default().fg(theme::DIM),
    )));
    frame.render_widget(Paragraph::new(footer).alignment(Alignment::Left), rows[2]);
}

fn draw_choices(frame: &mut ratatui::Frame, state: &mut State, area: Rect) {
    let visible = area.height.max(1) as usize;
    if state.choice < state.scroll {
        state.scroll = state.choice;
    } else if state.choice >= state.scroll + visible {
        state.scroll = state.choice + 1 - visible;
    }

    let mut lines = Vec::new();
    for (index, preset) in presets::PRESETS
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(visible)
    {
        let selected = index == state.choice;
        let marker = if selected { "▸ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::ASSISTANT)
        };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(theme::ACCENT)),
            Span::styled(format!("{:<28}", preset.label), style),
            Span::styled(preset.hint, Style::default().fg(theme::DIM)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn draw_input(frame: &mut ratatui::Frame, state: &State, area: Rect) {
    let (label, value, masked) = match state.step {
        Step::BaseUrl => ("base URL", &state.base_url, false),
        Step::Key => ("api key", &state.key, true),
        Step::Model => ("model", &state.model, false),
        Step::Provider => ("", &state.base_url, false),
    };
    let shown = if masked {
        "•".repeat(value.chars().count())
    } else {
        value.clone()
    };
    let placeholder = match state.step {
        Step::Model => "auto for local servers",
        Step::Key => "sk-…",
        _ => "https://…",
    };

    let mut lines = vec![
        Line::from(Span::styled(label, Style::default().fg(theme::DIM))),
        Line::from(""),
        Line::from(vec![
            Span::styled("› ", Style::default().fg(theme::ACCENT)),
            Span::styled(
                if shown.is_empty() {
                    placeholder.to_string()
                } else {
                    shown
                },
                if value.is_empty() {
                    Style::default().fg(theme::DIM)
                } else {
                    Style::default()
                        .fg(theme::ASSISTANT)
                        .add_modifier(Modifier::BOLD)
                },
            ),
        ]),
    ];
    if state.step == Step::Model {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "examples: claude-sonnet-4-5 · gpt-4o-mini · deepseek-chat · qwen2.5-7b-instruct",
            Style::default().fg(theme::DIM),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.max(20).min(area.width);
    let height = height.max(6).min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn select(name: &str) -> State {
        let mut state = State::new();
        state.choice = presets::PRESETS
            .iter()
            .position(|preset| preset.name == name)
            .unwrap();
        state
    }

    #[test]
    fn cloud_provider_walks_provider_key_model() {
        let mut state = State::new();
        assert!(matches!(
            state.handle_key(key(KeyCode::Enter)),
            Action::Continue
        ));
        assert_eq!(state.step, Step::Key);

        for ch in "sk-ant-123456".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        assert!(matches!(
            state.handle_key(key(KeyCode::Enter)),
            Action::Continue
        ));
        assert_eq!(state.step, Step::Model);

        match state.handle_key(key(KeyCode::Enter)) {
            Action::Done(config, name) => {
                assert_eq!(name, "anthropic");
                assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-5"));
                assert_eq!(config.api_key.as_deref(), Some("sk-ant-123456"));
                assert!(config.base_url.is_none());
            }
            _ => panic!("expected the wizard to finish"),
        }
    }

    #[test]
    fn local_provider_skips_the_key_step() {
        let mut state = select("lmstudio");
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.step, Step::Model);
        assert_eq!(state.base_url, "http://127.0.0.1:1234/v1");

        match state.handle_key(key(KeyCode::Enter)) {
            Action::Done(config, name) => {
                assert_eq!(name, "lmstudio");
                assert_eq!(config.model.as_deref(), Some("auto"));
                assert!(config.api_key.is_none());
            }
            _ => panic!("expected the wizard to finish"),
        }
    }

    #[test]
    fn third_party_presets_keep_their_endpoint() {
        let mut state = select("deepseek");
        state.handle_key(key(KeyCode::Enter));
        for ch in "sk-deepseek-key".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        state.handle_key(key(KeyCode::Enter));
        state.handle_key(key(KeyCode::Enter));

        match state.handle_key(key(KeyCode::Enter)) {
            Action::Done(config, name) => {
                assert_eq!(name, "deepseek");
                assert_eq!(
                    config.base_url.as_deref(),
                    Some("https://api.deepseek.com/v1")
                );
                assert_eq!(config.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
                assert_eq!(config.model.as_deref(), Some("deepseek-chat"));
            }
            _ => panic!("expected the wizard to finish"),
        }
    }

    #[test]
    fn custom_provider_requires_a_base_url() {
        let mut state = select("custom");
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.step, Step::BaseUrl);

        assert!(matches!(
            state.handle_key(key(KeyCode::Enter)),
            Action::Continue
        ));
        assert!(state.error.is_some());
        assert_eq!(state.step, Step::BaseUrl);

        for ch in "http://10.0.0.5:8080/v1".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.step, Step::Key);

        // an empty key is fine for custom endpoints
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.step, Step::Model);
    }

    #[test]
    fn empty_key_is_rejected_for_cloud_providers() {
        let mut state = State::new();
        state.handle_key(key(KeyCode::Enter));
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.step, Step::Key);
        assert!(state.error.is_some());
    }

    #[test]
    fn esc_from_the_first_step_cancels() {
        let mut state = State::new();
        assert!(matches!(
            state.handle_key(key(KeyCode::Esc)),
            Action::Cancel
        ));
    }

    #[test]
    fn backspace_edits_the_current_field() {
        let mut state = State::new();
        state.handle_key(key(KeyCode::Enter));
        for ch in "abc".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        state.handle_key(key(KeyCode::Backspace));
        assert_eq!(state.key, "ab");
    }

    #[test]
    fn typing_on_the_provider_step_does_not_touch_the_key() {
        let mut state = State::new();
        state.handle_key(key(KeyCode::Char('x')));
        assert!(state.key.is_empty());
    }

    fn screen(state: &mut State, width: u16, height: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, state)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn draws_the_provider_list() {
        let mut state = State::new();
        let text = screen(&mut state, 90, 30);
        assert!(text.contains("mybot setup"));
        assert!(text.contains("Choose a provider"));
        assert!(text.contains("Anthropic"));
        assert!(text.contains("DeepSeek"));
    }

    #[test]
    fn draws_the_key_step_masked() {
        let mut state = State::new();
        state.handle_key(key(KeyCode::Enter));
        for ch in "sk-secret-value".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        let text = screen(&mut state, 90, 30);
        assert!(text.contains("api key"));
        assert!(text.contains("••••"));
        assert!(!text.contains("sk-secret-value"));
    }

    #[test]
    fn list_scrolls_to_keep_the_selection_visible() {
        let mut state = State::new();
        for _ in 0..presets::PRESETS.len() - 1 {
            state.handle_key(key(KeyCode::Down));
        }
        let last = state.choice().label;
        let text = screen(&mut state, 90, 24);
        assert!(
            text.contains(last),
            "the selected provider should be on screen"
        );
    }
}
