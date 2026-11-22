use super::*;
use crate::agent::{CompactOutcome, CompactReport};
use crate::protocol::StopReason;

fn app() -> App {
    App::new(
        Status {
            provider: "mock".into(),
            model: "mock-model".into(),
            permission: crate::config::PermissionMode::Auto,
            session: "abc12345".into(),
            input_tokens: 0,
            output_tokens: 0,
        },
        PathBuf::from("/tmp"),
        None,
    )
}

#[test]
fn streaming_becomes_assistant_entry() {
    let mut app = app();
    app.handle_agent(AgentEvent::TurnStart { step: 1 });
    app.handle_agent(AgentEvent::TextDelta("hello".into()));
    app.handle_agent(AgentEvent::TurnEnd {
        stop_reason: StopReason::EndTurn,
    });
    assert!(matches!(
        app.transcript.last(),
        Some(Entry::Assistant(text)) if text == "hello"
    ));

    app.handle_agent(AgentEvent::Done {
        stop_reason: StopReason::EndTurn,
    });
    assert!(!app.busy);
}

#[test]
fn tool_lifecycle_updates_entry() {
    let mut app = app();
    app.handle_agent(AgentEvent::ToolCall {
        id: "c1".into(),
        name: "read".into(),
        summary: "read a.txt".into(),
    });
    app.handle_agent(AgentEvent::ToolStart {
        id: "c1".into(),
        name: "read".into(),
    });
    app.handle_agent(AgentEvent::ToolEnd {
        id: "c1".into(),
        name: "read".into(),
        content: "data".into(),
        is_error: false,
    });

    match app.transcript.last() {
        Some(Entry::Tool {
            content,
            running,
            is_error,
            ..
        }) => {
            assert_eq!(content, "data");
            assert!(!running);
            assert!(!is_error);
        }
        _ => panic!("expected a tool entry"),
    }
}

#[test]
fn permission_modal_answers_allow() {
    let mut app = app();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    app.set_permission(
        crate::permissions::PermissionRequest {
            tool: "bash".into(),
            summary: "run ls".into(),
            detail: "ls".into(),
            always_rule: None,
        },
        tx,
    );

    let consumed = app.handle_permission_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(consumed);
    assert_eq!(rx.try_recv().unwrap(), Decision::AllowOnce);
    assert!(app.permission.is_none());
}

#[test]
fn permission_modal_denies_on_escape() {
    let mut app = app();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    app.set_permission(
        crate::permissions::PermissionRequest {
            tool: "bash".into(),
            summary: "run rm".into(),
            detail: "rm x".into(),
            always_rule: None,
        },
        tx,
    );

    app.handle_permission_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(rx.try_recv().unwrap(), Decision::Deny);
}

#[test]
fn retry_event_shows_transient_info() {
    let mut app = app();
    app.handle_agent(AgentEvent::Retry {
        attempt: 1,
        max_retries: 2,
        delay_ms: 500,
        error: "HTTP 429: slow down".into(),
    });

    let (text, _) = app.info.clone().expect("retry should set the status message");
    assert!(text.contains("retrying 1/2"), "got {text}");
    assert!(text.contains("HTTP 429"), "got {text}");
    assert!(app.transcript.is_empty(), "retries must not clutter the transcript");
}

#[test]
fn done_event_clears_busy() {
    let mut app = app();
    app.busy = true;
    app.handle_agent(AgentEvent::Done {
        stop_reason: StopReason::EndTurn,
    });
    assert!(!app.busy);
}

#[test]
fn compact_events_clear_busy_and_add_boundary() {
    let mut app = app();
    app.busy = true;

    app.handle_agent(AgentEvent::CompactStart {
        messages: 6,
        tokens: 120,
    });
    let (info, _) = app.info.clone().expect("info should be set");
    assert!(info.contains("compacting 6 messages"), "got {info}");

    app.handle_agent(AgentEvent::CompactEnd(CompactOutcome::Compacted(
        CompactReport {
            messages_before: 6,
            messages_after: 1,
            tokens_before: 120,
            tokens_after: 15,
            summary_chars: 60,
            truncated: false,
        },
    )));
    assert!(!app.busy);

    match app.transcript.last() {
        Some(Entry::Info(text)) => {
            assert!(text.contains("compacted 6 messages"), "got {text}");
            assert!(text.contains("~120 → ~15 tokens"), "got {text}");
        }
        _ => panic!("expected compact boundary info"),
    }
}

#[test]
fn compact_empty_and_cancelled_clear_busy() {
    let mut app = app();
    app.busy = true;
    app.handle_agent(AgentEvent::CompactEnd(CompactOutcome::Empty));
    assert!(!app.busy);
    assert!(app.info.as_ref().unwrap().0.contains("nothing to compact"));

    app.busy = true;
    app.handle_agent(AgentEvent::CompactEnd(CompactOutcome::Cancelled));
    assert!(!app.busy);
    assert!(app.info.as_ref().unwrap().0.contains("cancelled"));
}
