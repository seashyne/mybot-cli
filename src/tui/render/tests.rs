use std::path::PathBuf;

use ratatui::text::Line;

use super::{draw, transcript_lines, transcript_lines_for, wrap_lines};
use crate::config::PermissionMode;
use crate::tui::app::{
    App, Entry, Mention, MentionItem, MentionKind, ModelItem, ModelPicker, Status,
};

fn app() -> App {
    App::new(
        Status {
            provider: "mock".into(),
            model: "mock-model".into(),
            permission: PermissionMode::Auto,
            thinking_budget: None,
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
fn draws_the_command_palette() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    app.composer.insert_str("/");
    app.sync_palette();

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
    assert!(text.contains("commands"));
    assert!(text.contains("/model"));
    assert!(text.contains("/permissions"));
}

#[test]
fn draws_the_model_picker_overlay() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    app.model_picker = Some(ModelPicker {
        all: vec![
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
        query: String::new(),
        selected: 1,
        error: None,
        loading: false,
        pending: 0,
        notes: Vec::new(),
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
    assert!(text.contains("type to filter"));
}

#[test]
fn welcome_card_borders_stay_aligned() {
    let app = app();
    let lines = transcript_lines_for(&app, 100);
    let box_lines: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .filter(|text| {
            let trimmed = text.trim_start();
            trimmed.starts_with('╭')
                || trimmed.starts_with('│')
                || trimmed.starts_with('├')
                || trimmed.starts_with('╰')
        })
        .collect();

    assert!(box_lines.len() >= 10);
    let expected_width = box_lines[0].chars().count();
    for row in &box_lines {
        assert_eq!(row.chars().count(), expected_width, "misaligned row: {row:?}");
        assert!(expected_width <= 100);
    }
    let joined = box_lines.join("\n");
    assert!(joined.contains("Ready"));
    assert!(joined.contains("Tips for getting started"));
    assert!(joined.contains("What's new"));
    assert!(joined.contains("/changelog"));
}

#[test]
fn narrow_welcome_screen_falls_back_cleanly() {
    let app = app();
    let text = flatten(&transcript_lines_for(&app, 42));
    assert!(text.contains("mybot cli"));
    assert!(text.contains("/help"));
    assert!(!text.contains('╭'));
}

#[test]
fn draws_status_bar_with_cwd_effort_context_and_full_access_red() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    let mut app = app();
    app.status.provider = "minimax".into();
    app.status.model = "M3.1-Flash".into();
    app.status.permission = PermissionMode::Full;
    app.status.thinking_budget = Some(32_768);

    let backend = TestBackend::new(110, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("/tmp"));
    assert!(text.contains("full access"));
    assert!(text.contains("effort high"));
    assert!(text.contains("context 1M"));
    assert!(text.contains("Ask mybot anything…"));
    assert!(text.contains("@ file or plugin"));

    let status_y = 29;
    let red_cells = (0..110)
        .filter(|&x| buffer[(x, status_y)].fg == Color::Red)
        .count();
    assert!(red_cells >= "full access".len());
}

#[test]
fn draws_the_mention_overlay() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    app.composer.insert_str("explain @ren");
    app.mention = Some(Mention::from_items(
        vec![
            MentionItem {
                kind: MentionKind::Plugin,
                label: "renderer".into(),
                insert: "plugin:renderer".into(),
                detail: "Render helpers".into(),
            },
            MentionItem {
                kind: MentionKind::File,
                label: "src/tui/render.rs".into(),
                insert: "src/tui/render.rs".into(),
                detail: String::new(),
            },
        ],
        8,
        "ren".into(),
    ));

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
    assert!(text.contains("@ files & plugins"));
    assert!(text.contains("[plugin]"));
    assert!(text.contains("src/tui/render.rs"));
}
