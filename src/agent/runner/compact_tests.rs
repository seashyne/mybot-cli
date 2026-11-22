use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::agent::CompactOutcome;
use crate::config::PermissionMode;
use crate::permissions::DenyPrompter;
use crate::providers::mock::MockProvider;
use crate::providers::{Provider, ProviderError, ProviderEvent};
use crate::protocol::StopReason;
use crate::tools::ToolRegistry;

fn agent(provider: Arc<dyn Provider>, dir: &std::path::Path) -> Agent {
    Agent::new(AgentOptions {
        provider,
        tools: Arc::new(ToolRegistry::with_defaults()),
        permissions: Arc::new(PermissionEngine::new(
            PermissionMode::Auto,
            dir.to_path_buf(),
            Vec::new(),
        )),
        prompter: Arc::new(DenyPrompter),
        system: vec!["test".into()],
        cwd: dir.to_path_buf(),
        max_tokens: 100,
        temperature: None,
        thinking_budget: None,
        max_steps: None,
        session: None,
        messages: Vec::new(),
        retry: RetryPolicy::default(),
    })
}

fn fast_retry() -> RetryPolicy {
    RetryPolicy {
        max_retries: 2,
        base_delay: std::time::Duration::from_millis(1),
        max_delay: std::time::Duration::from_millis(2),
    }
}

#[tokio::test]
async fn compact_replaces_history_with_summary() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("summary text"));
    let mut agent = agent(provider.clone(), dir.path());
    agent.messages = vec![
        Message::user("old question"),
        Message::assistant_text("old answer"),
    ];

    let (tx, mut rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let outcome = agent.compact(&tx, &cancel).await.unwrap();

    let report = match outcome {
        CompactOutcome::Compacted(report) => report,
        other => panic!("expected compacted, got {other:?}"),
    };
    assert_eq!(report.messages_before, 2);
    assert_eq!(report.messages_after, 1);
    assert!(!report.truncated);
    assert_eq!(provider.calls(), 1);
    assert_eq!(agent.messages().len(), 1);
    assert!(crate::compact::is_summary_message(&agent.messages()[0]));
    assert!(agent.messages()[0].text().contains("summary text"));

    let mut starts = 0;
    let mut ends = 0;
    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::CompactStart { messages, .. } => {
                starts += 1;
                assert_eq!(messages, 2);
            }
            AgentEvent::CompactEnd(CompactOutcome::Compacted(_)) => ends += 1,
            _ => {}
        }
    }
    assert_eq!((starts, ends), (1, 1));
}

#[tokio::test]
async fn compact_empty_history_does_not_call_provider() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("unused"));
    let mut agent = agent(provider.clone(), dir.path());

    let (tx, _rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let outcome = agent.compact(&tx, &cancel).await.unwrap();

    assert!(matches!(outcome, CompactOutcome::Empty));
    assert_eq!(provider.calls(), 0);
    assert!(agent.messages().is_empty());
}

#[tokio::test]
async fn compact_builds_single_flattened_request() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("summary"));
    let mut agent = agent(provider.clone(), dir.path());
    agent.messages = vec![
        Message::user("FIRST GOAL"),
        Message::assistant_text("did work"),
    ];

    let (tx, _rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    agent.compact(&tx, &cancel).await.unwrap();

    let request = provider.last_request().expect("request was captured");
    assert!(request.tools.is_empty());
    assert_eq!(request.messages.len(), 1);
    assert!(request.system[0].contains("handoff summary"));
    let text = request.messages[0].text();
    assert!(text.contains("FIRST GOAL"));
    assert!(text.contains("did work"));
    assert!(text.contains(crate::compact::INSTRUCTION));
}

#[tokio::test]
async fn compact_keeps_history_on_empty_summary() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::new(vec![vec![
        ProviderEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        },
    ]]));
    let mut agent = agent(provider, dir.path());
    agent.messages = vec![Message::user("keep me")];

    let (tx, mut rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    assert!(agent.compact(&tx, &cancel).await.is_err());

    assert_eq!(agent.messages().len(), 1);
    assert_eq!(agent.messages()[0].text(), "keep me");

    let mut saw_error = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, AgentEvent::Error(_)) {
            saw_error = true;
        }
    }
    assert!(saw_error);
}

#[tokio::test]
async fn compact_marks_truncated_summary() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::new(vec![vec![
        ProviderEvent::TextDelta("partial summary".into()),
        ProviderEvent::MessageStop {
            stop_reason: StopReason::MaxTokens,
        },
    ]]));
    let mut agent = agent(provider, dir.path());
    agent.messages = vec![Message::user("long history")];

    let (tx, _rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let outcome = agent.compact(&tx, &cancel).await.unwrap();

    match outcome {
        CompactOutcome::Compacted(report) => assert!(report.truncated),
        other => panic!("expected compacted, got {other:?}"),
    }
    assert!(agent.messages()[0].text().contains("partial summary"));
}

#[tokio::test]
async fn compact_retries_transient_failure() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::with_failures(
        vec![ProviderError::from_status(429, "{}")],
        vec![vec![
            ProviderEvent::TextDelta("summary".into()),
            ProviderEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            },
        ]],
    ));
    let mut agent = agent(provider.clone(), dir.path());
    agent.retry = fast_retry();
    agent.messages = vec![Message::user("history")];

    let (tx, _rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let outcome = agent.compact(&tx, &cancel).await.unwrap();

    assert!(matches!(outcome, CompactOutcome::Compacted(_)));
    assert_eq!(provider.calls(), 2, "one failure plus one retry");
}

#[tokio::test]
async fn compact_cancel_before_request() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("unused"));
    let mut agent = agent(provider.clone(), dir.path());
    agent.messages = vec![Message::user("history")];

    let (tx, _rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = agent.compact(&tx, &cancel).await.unwrap();

    assert!(matches!(outcome, CompactOutcome::Cancelled));
    assert_eq!(provider.calls(), 0);
    assert_eq!(agent.messages().len(), 1);
}
