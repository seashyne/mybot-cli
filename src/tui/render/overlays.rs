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

    let rows = picker.items.len() + 1; // + custom row
    let width = area.width.saturating_sub(8).clamp(30, 76);
    let height = (rows as u16 + 6).min(area.height.saturating_sub(2)).max(8);
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
    for (index, item) in picker.items.iter().enumerate() {
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
            Span::styled(format!("{:<12}", item.provider), name_style),
            Span::styled(
                format!("{:<28}", item.model),
                Style::default().fg(theme::ASSISTANT),
            ),
            Span::styled(tag.0, Style::default().fg(tag.1)),
        ]));
    }

    let custom_selected = picker.selected >= picker.items.len();
    let custom_marker = if custom_selected { "▸ " } else { "  " };
    match &picker.custom {
        Some(buffer) => {
            lines.push(Line::from(vec![
                Span::styled(custom_marker, Style::default().fg(theme::ACCENT)),
                Span::styled(
                    "custom      ",
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("› ", Style::default().fg(theme::ACCENT)),
                Span::styled(buffer.clone(), Style::default().fg(theme::ASSISTANT)),
                Span::styled("█", Style::default().fg(theme::ACCENT)),
            ]));
        }
        None => {
            lines.push(Line::from(vec![
                Span::styled(custom_marker, Style::default().fg(theme::ACCENT)),
                Span::styled(
                    "custom      type any model name…",
                    Style::default().fg(if custom_selected {
                        theme::ACCENT
                    } else {
                        theme::DIM
                    }),
                ),
            ]));
        }
    }

    lines.push(Line::from(""));
    if let Some(error) = &picker.error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(theme::ERROR),
        )));
    }
    lines.push(Line::from(Span::styled(
        if picker.custom.is_some() {
            "Enter apply · Esc back"
        } else {
            "↑↓ move · Enter apply · Esc close"
        },
        Style::default().fg(theme::DIM),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
