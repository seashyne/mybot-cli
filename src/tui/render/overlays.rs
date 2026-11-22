use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use crate::tui::app::App;
use crate::tui::theme;

pub(super) fn draw_permission(frame: &mut Frame, app: &App, area: Rect) {
    let Some(modal) = &app.permission else {
        return;
    };

    let width = area.width.saturating_sub(8).clamp(20, 80);
    let height = area.height.saturating_sub(4).clamp(6, 18);
    let rect = centered(area, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::TOOL))
        .title(" permission required ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut lines = vec![
        Line::from(Span::styled(
            modal.request.summary.clone(),
            Style::default()
                .fg(theme::TOOL)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for line in modal.request.detail.lines().take(8) {
        lines.push(Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(theme::ASSISTANT),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "a allow once · A always allow · d deny",
        Style::default().fg(theme::DIM),
    )));

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inner,
    );
}

pub(super) fn draw_model_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some(picker) = &app.model_picker else {
        return;
    };
    let visible = picker.visible();

    let width = area.width.saturating_sub(8).clamp(40, 92);
    let height = (visible.len() as u16 + 6)
        .min(area.height.saturating_sub(2))
        .max(9);
    let rect = centered(area, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(Span::styled(
            " switch model ",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("search ", Style::default().fg(theme::DIM)),
        Span::styled(
            if picker.query.is_empty() {
                "type to filter…".to_string()
            } else {
                picker.query.clone()
            },
            Style::default().fg(if picker.query.is_empty() {
                theme::DIM
            } else {
                theme::ASSISTANT
            }),
        ),
        Span::styled(
            if picker.query.is_empty() { "" } else { "█" },
            Style::default().fg(theme::ACCENT),
        ),
        Span::styled(
            if picker.loading {
                format!("  ({} models · loading…)", visible.len())
            } else {
                format!("  ({} models)", visible.len())
            },
            Style::default().fg(theme::DIM),
        ),
    ]));
    lines.push(Line::from(""));

    let rows = inner.height.saturating_sub(4) as usize;
    let start = if rows > 0 && picker.selected >= rows {
        picker.selected + 1 - rows
    } else {
        0
    };

    if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            if picker.all.is_empty() {
                "no providers configured — run `mybot provider add`"
            } else {
                "no matching model"
            },
            Style::default().fg(theme::DIM),
        )));
    } else {
        for (index, item) in visible.iter().enumerate().skip(start).take(rows) {
            let selected = index == picker.selected;
            let current = item.provider == app.status.provider && item.model == app.status.model;
            let marker = if selected { "▸ " } else { "  " };
            let name_style = if selected {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::ASSISTANT)
            };
            let tag = if current {
                (" current", theme::CODE)
            } else if item.ready {
                ("", theme::DIM)
            } else {
                (" needs key", theme::ERROR)
            };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(theme::ACCENT)),
                Span::styled(format!("{:<12}", item.provider), Style::default().fg(theme::DIM)),
                Span::styled(format!("{:<32}", item.model), name_style),
                Span::styled(tag.0, Style::default().fg(tag.1)),
            ]));
        }
    }

    while lines.len() < inner.height.saturating_sub(1) as usize {
        lines.push(Line::from(""));
    }
    if let Some(error) = &picker.error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(theme::ERROR),
        )));
    } else if let Some(note) = picker.notes.last() {
        lines.push(Line::from(Span::styled(
            truncate(note, inner.width as usize),
            Style::default().fg(theme::TOOL),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            if visible.is_empty() && !picker.query.trim().is_empty() {
                format!("Enter uses '{}' as a model id · Esc clear", picker.query.trim())
            } else {
                "↑↓ move · Enter apply · Esc close · type to filter".to_string()
            },
            Style::default().fg(theme::DIM),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw_palette(frame: &mut Frame, app: &App, area: Rect) {
    let matches = app.palette_matches();
    let selected = app
        .palette
        .as_ref()
        .map(|palette| palette.selected)
        .unwrap_or(0);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(Span::styled(
            " commands ",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = inner.height.saturating_sub(1) as usize;
    let start = if rows > 0 && selected >= rows {
        selected + 1 - rows
    } else {
        0
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    if matches.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no matching command — Enter runs it as typed",
            Style::default().fg(theme::DIM),
        )));
    } else {
        let description_width = inner.width.saturating_sub(26) as usize;
        for (index, command) in matches.iter().enumerate().skip(start).take(rows) {
            let is_selected = index == selected;
            let usage = if command.args.is_empty() {
                format!("/{}", command.name)
            } else {
                format!("/{} {}", command.name, command.args)
            };
            let marker = if is_selected { "▸ " } else { "  " };
            let suffix = if command.idle_only && app.busy {
                " · waits for the turn"
            } else {
                ""
            };
            let name_style = if is_selected {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::ASSISTANT)
            };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(theme::ACCENT)),
                Span::styled(format!("{usage:<24}"), name_style),
                Span::styled(
                    truncate(&format!("{}{suffix}", command.description), description_width),
                    Style::default().fg(theme::DIM),
                ),
            ]));
        }
    }

    while lines.len() < rows {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "↑↓ move · Tab complete · Enter run · Esc close",
        Style::default().fg(theme::DIM),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

fn truncate(text: &str, width: usize) -> String {
    if width == 0 || text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}
