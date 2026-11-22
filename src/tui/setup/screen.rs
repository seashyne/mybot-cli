use std::io::Stdout;

use crossterm::event::Event;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::error::{AppError, Result};
use crate::providers::presets;
use crate::tui::theme;

use super::{Action, State, Step};

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

pub(super) fn draw(frame: &mut ratatui::Frame, state: &mut State) {
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
