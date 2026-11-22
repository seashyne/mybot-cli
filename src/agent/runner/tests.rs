use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::*;
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
async fn single_turn_without_tools() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("hello"));
    let mut agent = agent(provider, dir.path());
    let (tx, mut rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();

    let stop = agent.run_turn("hi", &tx, &cancel).await.unwrap();
    assert_eq!(stop, StopReason::EndTurn);
    assert_eq!(agent.messages().len(), 2);
    assert!(agent.messages()[1].text().contains("hello"));

    let mut saw_text = false;
    while let Ok(event) = rx.try_recv() {
        if let AgentEvent::TextDelta(text) = event {
            saw_text = text == "hello";
        }
    }
    assert!(saw_text);
}

#[tokio::test]
async fn executes_tool_then_answers() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "content").unwrap();

    let provider = Arc::new(MockProvider::new(vec![
        vec![
            ProviderEvent::ToolCallStart {
                index: 0,
                id: "call_1".into(),
                name: "read".into(),
            },
            ProviderEvent::ToolCallArgsDelta {
                index: 0,
                json: "{\"path\":\"note.txt\"}".into(),
            },
            ProviderEvent::MessageStop {
                stop_reason: StopReason::ToolUse,
            },
        ],
        vec![
            ProviderEvent::TextDelta("done".into()),
            ProviderEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            },
        ],
    ]));

    let mut agent = agent(provider, dir.path());
    let (tx, _rx) = mpsc::channel(64);
    let cancel = CancellationToken::new();

    let stop = agent.run_turn("read note", &tx, &cancel).await.unwrap();
    assert_eq!(stop, StopReason::EndTurn);
    // user, assistant(tool call), tool results, assistant(text)
    assert_eq!(agent.messages().len(), 4);
    match &agent.messages()[2] {
        Message::ToolResults { results } => {
            assert_eq!(results.len(), 1);
            assert!(results[0].content.contains("content"));
            assert!(!results[0].is_error);
        }
        other => panic!("expected tool results, got {other:?}"),
    }
}

#[tokio::test]
async fn retries_transient_failures_then_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::with_failures(
        vec![
            ProviderError::from_status(429, r#"{"error":{"message":"slow down"}}"#),
            ProviderError::from_status(503, "{}"),
        ],
        vec![vec![
            ProviderEvent::TextDelta("recovered".into()),
            ProviderEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            },
        ]],
    ));
    let mut agent = agent(provider.clone(), dir.path());
    agent.retry = fast_retry();

    let (tx, mut rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let stop = agent.run_turn("hi", &tx, &cancel).await.unwrap();

    assert_eq!(stop, StopReason::EndTurn);
    assert_eq!(provider.calls(), 3);
    assert!(agent.messages()[1].text().contains("recovered"));

    let mut retries = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AgentEvent::Retry {
            attempt,
            max_retries,
            ..
        } = event
        {
            retries.push((attempt, max_retries));
        }
    }
    assert_eq!(retries, vec![(1, 2), (2, 2)]);
}

#[tokio::test]
async fn does_not_retry_client_errors() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::with_failures(
        vec![ProviderError::from_status(400, r#"{"error":{"message":"bad request"}}"#)],
        Vec::new(),
    ));
    let mut agent = agent(provider.clone(), dir.path());
    agent.retry = fast_retry();

    let (tx, _rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let error = agent.run_turn("hi", &tx, &cancel).await.unwrap_err();

    assert_eq!(error.exit_code(), 3);
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn gives_up_after_max_retries() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::with_failures(
        vec![
            ProviderError::from_status(500, "{}"),
            ProviderError::from_status(500, "{}"),
            ProviderError::from_status(500, "{}"),
        ],
        Vec::new(),
    ));
    let mut agent = agent(provider.clone(), dir.path());
    agent.retry = fast_retry();

    let (tx, _rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let error = agent.run_turn("hi", &tx, &cancel).await.unwrap_err();

    assert_eq!(error.exit_code(), 4);
    assert_eq!(provider.calls(), 3, "one attempt plus two retries");
}

#[tokio::test]
async fn cancel_stops_retry_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::with_failures(
        vec![
            ProviderError::from_status(500, "{}"),
            ProviderError::from_status(500, "{}"),
        ],
        Vec::new(),
    ));
    let mut agent = agent(provider.clone(), dir.path());
    agent.retry = RetryPolicy {
        max_retries: 5,
        base_delay: std::time::Duration::from_secs(30),
        max_delay: std::time::Duration::from_secs(60),
    };

    let (tx, mut rx) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        token.cancel();
    });

    let stop = agent.run_turn("hi", &tx, &cancel).await.unwrap();
    assert_eq!(stop, StopReason::Aborted);
    assert_eq!(provider.calls(), 1, "no retry after cancellation");

    let mut saw_abort = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(
            event,
            AgentEvent::TurnEnd {
                stop_reason: StopReason::Aborted
            }
        ) {
            saw_abort = true;
        }
    }
    assert!(saw_abort);
}

#[tokio::test]
async fn respects_max_steps() {
    let dir = tempfile::tempdir().unwrap();
    let script: Vec<Vec<ProviderEvent>> = (0..5)
        .map(|_| {
            vec![
                ProviderEvent::ToolCallStart {
                    index: 0,
                    id: "c".into(),
                    name: "read".into(),
                },
                ProviderEvent::ToolCallArgsDelta {
                    index: 0,
                    json: "{\"path\":\"missing\"}".into(),
                },
                ProviderEvent::MessageStop {
                    stop_reason: StopReason::ToolUse,
                },
            ]
        })
        .collect();
    let provider = Arc::new(MockProvider::new(script));
    let mut agent = agent(provider, dir.path());
    agent.max_steps = Some(2);

    let (tx, _rx) = mpsc::channel(64);
    let cancel = CancellationToken::new();
    let error = agent.run_turn("loop", &tx, &cancel).await.unwrap_err();
    assert_eq!(error.exit_code(), 7);
}
