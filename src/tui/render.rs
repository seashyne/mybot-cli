use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use super::app::{App, Entry};
use super::{markdown, theme, todo};

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
    let composer_height = (app.composer.lines().len() as u16).clamp(1, 6) + 2;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(todo_height),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(area);

    draw_transcript(frame, app, chunks[0]);
    if todo_height > 0 {
        draw_todos(frame, app, chunks[1]);
    }
    draw_composer(frame, app, chunks[2]);
    draw_status(frame, app, chunks[3]);

    if app.permission.is_some() {
        draw_permission(frame, app, area);
    }
    if app.model_picker.is_some() {
        draw_model_picker(frame, app, area);
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

fn draw_permission(frame: &mut Frame, app: &App, area: Rect) {
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

fn draw_model_picker(frame: &mut Frame, app: &App, area: Rect) {
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

fn centered(area: Rect, width: u16, height: u16) -> Rect {    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionMode;
    use crate::tui::app::Status;
    use std::path::PathBuf;

    fn app() -> App {
        App::new(
            Status {
                provider: "mock".into(),
                model: "mock-model".into(),
                permission: PermissionMode::Auto,
                session: "abc12345".into(),
                input_tokens: 0,
                output_tokens: 0,
            },
            PathBuf::from("/tmp"),
            None,
        )
    }

    fn flatten(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn welcome_screen_mentions_help() {
        let app = app();
        let text = flatten(&transcript_lines(&app));
        assert!(text.contains("mybot cli"));
        assert!(text.contains("/help"));
    }

    #[test]
    fn renders_user_assistant_and_tool_entries() {
        let mut app = app();
        app.transcript.push(Entry::User("hello".into()));
        app.transcript
            .push(Entry::Assistant("**hi** there".into()));
        app.transcript.push(Entry::Tool {
            id: "1".into(),
            name: "read".into(),
            summary: "read a.txt".into(),
            content: "ok".into(),
            is_error: false,
            running: false,
            expanded: false,
        });

        let text = flatten(&transcript_lines(&app));
        assert!(text.contains("› hello"));
        assert!(text.contains("hi there"));
        assert!(text.contains("read a.txt"));
    }

    #[test]
    fn wrap_splits_long_lines() {
        let lines = vec![Line::from(Span::raw("abcdefghij"))];
        let wrapped = wrap_lines(lines, 4);
        assert_eq!(wrapped.len(), 3);
        assert_eq!(flatten(&wrapped), "abcd\nefgh\nij");
    }

    #[test]
    fn draws_assistant_text_to_terminal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = app();
        app.transcript
            .push(Entry::Assistant("Hello from fake provider".into()));

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        let text: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            text.contains("Hello from fake provider"),
            "buffer did not contain the answer: {text:?}"
        );
    }

    #[test]
    fn draws_permission_modal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = app();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        app.set_permission(
            crate::permissions::PermissionRequest {
                tool: "bash".into(),
                summary: "run a command".into(),
                detail: "rm -rf build".into(),
                always_rule: None,
            },
            tx,
        );

        let backend = TestBackend::new(90, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        let text: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("permission required"));
        assert!(text.contains("rm -rf build"));
    }

    #[test]
    fn error_entry_is_marked() {
        let mut app = app();
        app.transcript.push(Entry::Error("boom".into()));
        let text = flatten(&transcript_lines(&app));
        assert!(text.contains("! boom"));
    }

    #[test]
    fn draws_the_model_picker_overlay() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = app();
        app.model_picker = Some(crate::tui::app::ModelPicker {
            items: vec![
                crate::tui::app::ModelItem {
                    provider: "anthropic".into(),
                    model: "claude-sonnet-4-5".into(),
                    ready: true,
                },
                crate::tui::app::ModelItem {
                    provider: "openai".into(),
                    model: "gpt-4o-mini".into(),
                    ready: false,
                },
            ],
            selected: 1,
            custom: None,
            error: None,
        });

        let backend = TestBackend::new(90, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("switch model"));
        assert!(text.contains("claude-sonnet-4-5"));
        assert!(text.contains("needs key"));
        assert!(text.contains("custom"));
    }
}
