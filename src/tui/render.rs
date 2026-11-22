use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use super::app::{App, Entry};
use super::{markdown, theme, todo};
use crate::team::MemberStatus;

mod overlays;
#[cfg(test)]
mod tests;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.height < 6 {
        return;
    }

    let todo_height = if app.todos.is_empty() {
        0
    } else {
        (app.todos.len() as u16 + 2).min(8)
    };
    let team_height = match &app.team {
        Some(snapshot) if !snapshot.members.is_empty() => {
            (snapshot.members.len().min(4) as u16) + 2
        }
        _ => 0,
    };
    let composer_height = (app.composer.lines().len() as u16).clamp(1, 6) + 2;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(todo_height),
            Constraint::Length(team_height),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(area);

    draw_transcript(frame, app, chunks[0]);
    if todo_height > 0 {
        draw_todos(frame, app, chunks[1]);
    }
    if team_height > 0 {
        draw_team(frame, app, chunks[2]);
    }
    draw_composer(frame, app, chunks[3]);
    draw_status(frame, app, chunks[4]);

    if app.permission.is_some() {
        overlays::draw_permission(frame, app, area);
    }
    if app.model_picker.is_some() {
        overlays::draw_model_picker(frame, app, area);
    }
}

const LOGO: [&str; 6] = [
    " ███╗   ███╗██╗   ██╗██████╗  ██████╗ ████████╗",
    " ████╗ ████║╚██╗ ██╔╝██╔══██╗██╔═══██╗╚══██╔══╝",
    " ██╔████╔██║ ╚████╔╝ ██████╔╝██║   ██║   ██║   ",
    " ██║╚██╔╝██║  ╚██╔╝  ██╔══██╗██║   ██║   ██║   ",
    " ██║ ╚═╝ ██║   ██║   ██████╔╝╚██████╔╝   ██║   ",
    " ╚═╝     ╚═╝   ╚═╝   ╚═════╝  ╚═════╝    ╚═╝   ",
];

fn transcript_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if app.transcript.is_empty() && app.streaming.is_empty() {
        for (index, row) in LOGO.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                row.to_string(),
                Style::default()
                    .fg(theme::GRADIENT[index.min(theme::GRADIENT.len() - 1)])
                    .add_modifier(Modifier::BOLD),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                "mybot cli",
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  ·  terminal coding agent",
                Style::default().fg(theme::DIM),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{}/{}", app.status.provider, app.status.model),
                Style::default()
                    .fg(theme::CODE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  permission {}", app.status.permission.label()),
                Style::default().fg(theme::DIM),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Type a request, or /help for commands.",
            Style::default().fg(theme::DIM),
        )));
        lines.push(Line::from(Span::styled(
            "  /model switches provider · /compact frees context · Esc cancels",
            Style::default().fg(theme::DIM),
        )));
        return lines;
    }

    for entry in &app.transcript {
        match entry {
            Entry::User(text) => {
                lines.push(Line::from(vec![
                    Span::styled("› ", Style::default().fg(theme::USER)),
                    Span::styled(
                        text.clone(),
                        Style::default()
                            .fg(theme::USER)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            Entry::Assistant(text) => lines.extend(markdown::render(text)),
            Entry::Tool {
                name,
                summary,
                content,
                is_error,
                running,
                expanded,
                ..
            } => {
                let (marker, color) = if *running {
                    ("◐", theme::TOOL)
                } else if *is_error {
                    ("✗", theme::ERROR)
                } else {
                    ("✓", theme::TOOL)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{marker} "), Style::default().fg(color)),
                    Span::styled(
                        name.clone(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  {summary}"), Style::default().fg(theme::DIM)),
                ]));
                if *expanded || *is_error {
                    let limit = if *expanded { 40 } else { 6 };
                    for line in content.lines().take(limit) {
                        lines.push(Line::from(Span::styled(
                            format!("   {line}"),
                            Style::default().fg(theme::DIM),
                        )));
                    }
                }
            }
            Entry::Info(text) => {
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(theme::DIM),
                    )));
                }
            }
            Entry::Error(text) => {
                lines.push(Line::from(Span::styled(
                    format!("! {text}"),
                    Style::default().fg(theme::ERROR),
                )));
            }
        }
        lines.push(Line::from(""));
    }

    if !app.streaming.is_empty() {
        lines.extend(markdown::render(&app.streaming));
    }

    lines
}

fn draw_transcript(frame: &mut Frame, app: &mut App, area: Rect) {
    let wrapped = wrap_lines(transcript_lines(app), area.width.max(1) as usize);
    let total = wrapped.len() as u16;
    let max_scroll = total.saturating_sub(area.height);

    if app.auto_scroll {
        app.scroll_from_bottom = 0;
    }
    let offset = app.scroll_from_bottom.min(max_scroll);
    let scroll = max_scroll.saturating_sub(offset);

    frame.render_widget(Paragraph::new(wrapped).scroll((scroll, 0)), area);
}

/// Pre-wrap styled lines to the viewport width so scrolling and the total line
/// count stay exact.
fn wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out = Vec::new();

    for line in lines {
        let mut current: Vec<Span<'static>> = Vec::new();
        let mut used = 0usize;

        for span in line.spans {
            let style = span.style;
            let mut chunk = String::new();
            for ch in span.content.chars() {
                if used == width {
                    if !chunk.is_empty() {
                        current.push(Span::styled(std::mem::take(&mut chunk), style));
                    }
                    out.push(Line::from(std::mem::take(&mut current)));
                    used = 0;
                }
                chunk.push(ch);
                used += 1;
            }
            if !chunk.is_empty() {
                current.push(Span::styled(chunk, style));
            }
        }

        out.push(Line::from(current));
    }

    out
}

fn draw_composer(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.busy {
        Style::default().fg(theme::DIM)
    } else {
        Style::default().fg(theme::ACCENT)
    };
    let title = if app.busy { " working… " } else { " message " };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = app.composer.lines().join("\n");
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);

    let (row, col) = app.composer.cursor();
    let x = inner.x + (col as u16).min(inner.width.saturating_sub(1));
    let y = inner.y + (row as u16).min(inner.height.saturating_sub(1));
    frame.set_cursor_position((x, y));
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    let mut spans = vec![
        Span::styled(
            format!(" {}/{} ", app.status.provider, app.status.model),
            Style::default().fg(theme::ACCENT),
        ),
        Span::styled(
            format!("[{}] ", app.status.permission.label()),
            Style::default().fg(theme::DIM),
        ),
        Span::styled(
            format!(
                "↑{} ↓{} ",
                app.status.input_tokens, app.status.output_tokens
            ),
            Style::default().fg(theme::DIM),
        ),
        Span::styled(
            format!("session {} ", app.status.session),
            Style::default().fg(theme::DIM),
        ),
    ];

    if app.busy {
        spans.push(Span::styled(
            format!("{} ", SPINNER[app.spinner % SPINNER.len()]),
            Style::default().fg(theme::TOOL),
        ));
    }

    if let Some((message, _)) = &app.info {
        spans.push(Span::styled(message.clone(), Style::default().fg(theme::TOOL)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_todos(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::BORDER))
        .title(" todos ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(todo::lines(&app.todos)), inner);
}

fn draw_team(frame: &mut Frame, app: &App, area: Rect) {
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
