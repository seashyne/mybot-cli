use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::{App, Entry};
use super::{markdown, theme};

mod chrome;
mod overlays;
#[cfg(test)]
mod tests;
mod welcome;

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
    let bar_height = if app.palette.is_some() {
        app.palette_matches().len().clamp(1, 8) as u16 + 3
    } else if app.mention.is_some() {
        app.mention_matches().len().clamp(1, 8) as u16 + 3
    } else if area.height >= 12 {
        1
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(todo_height),
            Constraint::Length(team_height),
            Constraint::Length(bar_height),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(area);

    draw_transcript(frame, app, chunks[0]);
    if todo_height > 0 {
        chrome::draw_todos(frame, app, chunks[1]);
    }
    if team_height > 0 {
        chrome::draw_team(frame, app, chunks[2]);
    }
    if app.palette.is_some() {
        overlays::draw_palette(frame, app, chunks[3]);
    } else if app.mention.is_some() {
        overlays::draw_mention(frame, app, chunks[3]);
    } else if bar_height > 0 {
        chrome::draw_composer_hint(frame, chunks[3]);
    }
    chrome::draw_composer(frame, app, chunks[4]);
    chrome::draw_status(frame, app, chunks[5]);

    if app.permission.is_some() {
        overlays::draw_permission(frame, app, area);
    }
    if app.model_picker.is_some() {
        overlays::draw_model_picker(frame, app, area);
    }
}

#[cfg(test)]
fn transcript_lines(app: &App) -> Vec<Line<'static>> {
    transcript_lines_for(app, 78)
}

fn transcript_lines_for(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if app.transcript.is_empty() && app.streaming.is_empty() {
        return welcome::lines(app, width);
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
                let mut header = vec![
                    Span::styled(format!("{marker} "), Style::default().fg(color)),
                    Span::styled(
                        name.clone(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  {summary}"), Style::default().fg(theme::DIM)),
                ];
                if !*running && !*expanded && !*is_error && !content.trim().is_empty() {
                    header.push(Span::styled("  (Ctrl+O)", Style::default().fg(theme::DIM)));
                }
                lines.push(Line::from(header));
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
    let width = area.width.max(1) as usize;
    let wrapped = wrap_lines(transcript_lines_for(app, width), width);
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
