use std::path::PathBuf;

use ratatui::text::Line;

use super::{draw, transcript_lines, wrap_lines};
use crate::config::PermissionMode;
use crate::tui::app::{App, Entry, ModelItem, ModelPicker, Status};

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
    let lines = vec![Line::from(ratatui::text::Span::raw("abcdefghij"))];
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
    app.model_picker = Some(ModelPicker {
        items: vec![
            ModelItem {
                provider: "anthropic".into(),
                model: "claude-sonnet-4-5".into(),
                ready: true,
            },
            ModelItem {
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
