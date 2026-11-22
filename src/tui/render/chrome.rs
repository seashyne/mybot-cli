use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use super::welcome;
use crate::config::EffortLevel;
use crate::providers::catalog;
use crate::team::MemberStatus;
use crate::tui::app::App;
use crate::tui::{theme, todo};

pub(super) fn draw_composer_hint(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            " Start",
            Style::default().fg(theme::USER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " · @ file or plugin · / autocomplete · Shift+Enter newline",
            Style::default().fg(theme::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

pub(super) fn draw_composer(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.busy {
        Style::default().fg(theme::DIM)
    } else {
        Style::default().fg(theme::ACCENT)
    };
    let title = if app.palette.is_some() {
        " command "
    } else if app.mention.is_some() {
        " mention "
    } else if app.busy {
        " working… "
    } else {
        " message "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text_area = if inner.width > 6 {
        let prompt_style = if app.busy {
            Style::default().fg(theme::DIM)
        } else {
            Style::default().fg(theme::USER).add_modifier(Modifier::BOLD)
        };
        let gutter = Rect {
            x: inner.x,
            y: inner.y,
            width: 2,
            height: 1.min(inner.height),
        };
        frame.render_widget(Paragraph::new(Span::styled("› ", prompt_style)), gutter);
        Rect {
            x: inner.x + 2,
            y: inner.y,
            width: inner.width - 2,
            height: inner.height,
        }
    } else {
        inner
    };

    if app.composer.is_empty() && !app.busy {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Ask mybot anything…",
                Style::default().fg(theme::DIM),
            )),
            text_area,
        );
        frame.set_cursor_position((text_area.x, text_area.y));
        return;
    }

    let text = app.composer.lines().join("\n");
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), text_area);

    let (row, col) = app.composer.cursor();
    let x = text_area.x + (col as u16).min(text_area.width.saturating_sub(1));
    let y = text_area.y + (row as u16).min(text_area.height.saturating_sub(1));
    frame.set_cursor_position((x, y));
}

pub(super) fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    let cwd_budget = if area.width >= 100 { 28 } else { 18 };
    let cwd = welcome::short_cwd(&app.cwd, cwd_budget);
    let effort = EffortLevel::from_budget(app.status.thinking_budget);
    let effort_style = if effort == EffortLevel::Off {
        Style::default().fg(theme::DIM)
    } else {
        Style::default().fg(theme::USER)
    };

    let mut spans = vec![
        Span::styled(format!(" {cwd} "), Style::default().fg(theme::DIM)),
        Span::styled("│ ", Style::default().fg(theme::BORDER)),
        Span::styled(
            welcome::permission_label(app.status.permission),
            welcome::permission_style(app.status.permission),
        ),
        Span::styled(" │ ", Style::default().fg(theme::BORDER)),
        Span::styled("✦ ", Style::default().fg(theme::USER)),
        Span::styled(
            format!("{}/{}", app.status.provider, app.status.model),
            Style::default().fg(theme::ACCENT),
        ),
        Span::styled(" · ", Style::default().fg(theme::DIM)),
        Span::styled(format!("effort {}", effort.label()), effort_style),
    ];

    if let Some(limit) = catalog::context_window(&app.status.model) {
        let ctx_text = if app.status.input_tokens > 0 {
            let pct = ((app.status.input_tokens as f64 / limit as f64) * 100.0).round() as u64;
            format!(
                "ctx {}/{} ({pct}%)",
                catalog::format_tokens(app.status.input_tokens),
                catalog::format_tokens(limit)
            )
        } else {
            format!("context {}", catalog::format_tokens(limit))
        };
        spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER)));
        spans.push(Span::styled(ctx_text, Style::default().fg(theme::DIM)));
    } else if app.status.input_tokens > 0 || app.status.output_tokens > 0 {
        spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER)));
        spans.push(Span::styled(
            format!(
                "↑{} ↓{}",
                catalog::format_tokens(app.status.input_tokens),
                catalog::format_tokens(app.status.output_tokens)
            ),
            Style::default().fg(theme::DIM),
        ));
    }

    if app.busy {
        spans.push(Span::styled(
            format!(" {}", SPINNER[app.spinner % SPINNER.len()]),
            Style::default().fg(theme::TOOL),
        ));
    }

    if let Some((message, _)) = &app.info {
        spans.push(Span::styled(" · ", Style::default().fg(theme::BORDER)));
        spans.push(Span::styled(message.clone(), Style::default().fg(theme::TOOL)));
    } else if !app.busy && area.width >= 105 {
        spans.push(Span::styled(
            format!(" · {}", app.status.session),
            Style::default().fg(theme::DIM),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub(super) fn draw_todos(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::BORDER))
        .title(" todos ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(todo::lines(&app.todos)), inner);
}

pub(super) fn draw_team(frame: &mut Frame, app: &App, area: Rect) {
    let Some(snapshot) = &app.team else {
        return;
    };
    let completed = snapshot
        .tasks
        .iter()
        .filter(|task| task.status == "completed")
        .count();
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::BORDER))
        .title(format!(
            " team · {} running · tasks {completed}/{} ",
            snapshot.running(),
            snapshot.tasks.len()
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    for member in snapshot.members.iter().take(4) {
        let (marker, color) = match member.status {
            MemberStatus::Running => ("◐", theme::TOOL),
            MemberStatus::Done => ("✓", theme::DIM),
            MemberStatus::Failed => ("✗", theme::ERROR),
        };
        let detail = match (&member.activity, member.status) {
            (Some(activity), MemberStatus::Running) => activity.clone(),
            _ => member.status.label().to_string(),
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), Style::default().fg(color)),
            Span::styled(
                format!("{:<14}", member.name),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{:<12}", member.role), Style::default().fg(theme::DIM)),
            Span::styled(detail, Style::default().fg(theme::DIM)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
