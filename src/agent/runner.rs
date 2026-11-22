use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, Result};
use crate::permissions::{PermissionEngine, Prompter};
use crate::protocol::{Message, StopReason, ToolResult, estimate_tokens};
use crate::providers::{CompletionRequest, EventStream, Provider, ProviderEvent, RetryPolicy};
use crate::session::Session;
use crate::tools::{ExecutionMode, TodoItem, ToolCtx, ToolRegistry};

use super::assembler::{Assembler, ToolInvocation};
use super::events::{AgentEvent, CompactOutcome, CompactReport};

pub struct AgentOptions {
    pub provider: Arc<dyn Provider>,
    pub tools: Arc<ToolRegistry>,
    pub permissions: Arc<PermissionEngine>,
    pub prompter: Arc<dyn Prompter>,
    pub system: Vec<String>,
    pub cwd: PathBuf,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub thinking_budget: Option<u32>,
    pub max_steps: Option<usize>,
    pub session: Option<Arc<Mutex<Session>>>,
    pub messages: Vec<Message>,
    pub retry: RetryPolicy,
}

/// Provider-side settings that can change while the agent is running.
pub struct ProviderSwap {
    pub provider: Arc<dyn Provider>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub thinking_budget: Option<u32>,
    pub retry: RetryPolicy,
}

pub struct Agent {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    permissions: Arc<PermissionEngine>,
    prompter: Arc<dyn Prompter>,
    system: Vec<String>,
    cwd: PathBuf,
    max_tokens: u32,
    temperature: Option<f32>,
    thinking_budget: Option<u32>,
    max_steps: Option<usize>,
    session: Option<Arc<Mutex<Session>>>,
    messages: Vec<Message>,
    todos: Arc<Mutex<Vec<TodoItem>>>,
    retry: RetryPolicy,
}

impl Agent {
    pub fn new(options: AgentOptions) -> Self {
        Self {
            provider: options.provider,
            tools: options.tools,
            permissions: options.permissions,
            prompter: options.prompter,
            system: options.system,
            cwd: options.cwd,
            max_tokens: options.max_tokens,
            temperature: options.temperature,
            thinking_budget: options.thinking_budget,
            max_steps: options.max_steps,
            session: options.session,
            messages: options.messages,
            todos: Arc::new(Mutex::new(Vec::new())),
            retry: options.retry,
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }

    pub fn model_name(&self) -> &str {
        self.provider.model()
    }

    pub fn todos(&self) -> Arc<Mutex<Vec<TodoItem>>> {
        self.todos.clone()
    }

    /// Swap the model client at runtime without touching the conversation.
    pub fn set_provider(&mut self, options: ProviderSwap) {
        self.provider = options.provider;
        self.max_tokens = options.max_tokens;
        self.temperature = options.temperature;
        self.thinking_budget = options.thinking_budget;
        self.retry = options.retry;
    }

    /// Run one user turn to completion, including any tool-call iterations.
    pub async fn run_turn(
        &mut self,
        input: &str,
        tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<StopReason> {
        self.record(Message::user(input)).await;

        let mut step = 0usize;
        let final_stop;

        loop {
            if cancel.is_cancelled() {
                let _ = tx
                    .send(AgentEvent::TurnEnd {
                        stop_reason: StopReason::Aborted,
                    })
                    .await;
                final_stop = StopReason::Aborted;
                break;
            }

            step += 1;
            if let Some(max) = self.max_steps
                && step > max {
                    let message = format!("stopped after {max} model turns");
                    let _ = tx.send(AgentEvent::Error(message.clone())).await;
                    return Err(AppError::Limit(message));
                }

            let _ = tx.send(AgentEvent::TurnStart { step }).await;

            let request = CompletionRequest {
                model: self.provider.model().to_string(),
                system: self.system.clone(),
                messages: self.messages.clone(),
                tools: self.tools.specs(),
                max_tokens: self.max_tokens,
                temperature: self.temperature,
                thinking_budget: self.thinking_budget,
            };

            let mut stream = match self.open_stream(request, tx, cancel).await {
                Ok(stream) => stream,
                Err(error) => {
                    if cancel.is_cancelled() {
                        let _ = tx
                            .send(AgentEvent::TurnEnd {
                                stop_reason: StopReason::Aborted,
                            })
                            .await;
                        final_stop = StopReason::Aborted;
                        break;
                    }
                    return Err(error);
                }
            };

            let mut assembler = Assembler::new();
            let mut stop_reason = StopReason::EndTurn;

            while let Some(item) = stream.next().await {
                match item {
                    Err(error) => {
                        let _ = tx.send(AgentEvent::Error(error.to_string())).await;
                        return Err(error.into());
                    }
                    Ok(event) => match event {
                        ProviderEvent::TextDelta(text) => {
                            assembler.on_text(&text);
                            let _ = tx.send(AgentEvent::TextDelta(text)).await;
                        }
                        ProviderEvent::ThinkingDelta(text) => {
                            assembler.on_thinking(&text);
                            let _ = tx.send(AgentEvent::ThinkingDelta(text)).await;
                        }
                        ProviderEvent::ThinkingSignature(signature) => {
                            assembler.on_signature(&signature);
                        }
                        ProviderEvent::ToolCallStart { index, id, name } => {
                            assembler.on_tool_start(index, &id, &name);
                        }
                        ProviderEvent::ToolCallArgsDelta { index, json } => {
                            assembler.on_tool_args(index, &json);
                        }
                        ProviderEvent::Usage { input, output } => {
                            let _ = tx.send(AgentEvent::Usage { input, output }).await;
                        }
                        ProviderEvent::MessageStop { stop_reason: reason } => {
                            stop_reason = reason;
                        }
                    },
                }
            }

            let assembled = assembler.finish();
            self.record(Message::Assistant {
                content: assembled.blocks,
            })
            .await;
            let _ = tx.send(AgentEvent::TurnEnd { stop_reason }).await;

            if stop_reason == StopReason::Aborted {
                final_stop = StopReason::Aborted;
                break;
            }

            if stop_reason == StopReason::Error || assembled.invocations.is_empty() {
                final_stop = stop_reason;
                break;
            }

            let results = self
                .execute_tools(&assembled.invocations, tx, cancel)
                .await;
            self.record(Message::ToolResults { results }).await;
        }

        let _ = tx
            .send(AgentEvent::ModelName(self.provider.model().to_string()))
            .await;
        let _ = tx
            .send(AgentEvent::Done {
                stop_reason: final_stop,
            })
            .await;
        Ok(final_stop)
    }

    /// Open the model stream, retrying transient failures (429, 5xx, connection
    /// errors) with exponential backoff.
    ///
    /// Only failures that happen before the first byte arrives are retried, so
    /// retrying can never duplicate streamed text.
    async fn open_stream(
        &self,
        request: CompletionRequest,
        tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<EventStream> {
        let mut attempt = 0u32;
        loop {
            match self.provider.stream(request.clone(), cancel.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    if cancel.is_cancelled() {
                        return Err(AppError::Cancelled);
                    }
                    if !error.is_retryable() || attempt >= self.retry.max_retries {
                        let _ = tx.send(AgentEvent::Error(error.to_string())).await;
                        return Err(error.into());
                    }
                    attempt += 1;
                    let delay = self.retry.delay(attempt, error.retry_after);
                    let _ = tx
                        .send(AgentEvent::Retry {
                            attempt,
                            max_retries: self.retry.max_retries,
                            delay_ms: delay.as_millis() as u64,
                            error: error.message.clone(),
                        })
                        .await;
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(AppError::Cancelled),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
    }

    /// Clear the conversation and optionally switch to a new session ledger.
    pub async fn reset(&mut self, session: Option<Arc<Mutex<Session>>>) {
        self.messages.clear();
        self.session = session;
        self.todos.lock().await.clear();
    }

    /// Summarize the conversation with the active provider and replace the
    /// history with that summary.
    ///
    /// The compaction is written to the session ledger before the in-memory
    /// history changes, so a failed write leaves the turn state untouched.
    pub async fn compact(
        &mut self,
        tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<CompactOutcome> {
        if self.messages.is_empty() {
            let _ = tx
                .send(AgentEvent::CompactEnd(CompactOutcome::Empty))
                .await;
            return Ok(CompactOutcome::Empty);
        }
        if cancel.is_cancelled() {
            let _ = tx
                .send(AgentEvent::CompactEnd(CompactOutcome::Cancelled))
                .await;
            return Ok(CompactOutcome::Cancelled);
        }

        let messages_before = self.messages.len();
        let tokens_before = estimate_tokens(&self.messages);
        let _ = tx
            .send(AgentEvent::CompactStart {
                messages: messages_before,
                tokens: tokens_before,
            })
            .await;

        let request = CompletionRequest {
            model: self.provider.model().to_string(),
            system: vec![crate::compact::SYSTEM_PROMPT.to_string()],
            messages: crate::compact::request_messages(&self.messages),
            tools: Vec::new(),
            max_tokens: self.max_tokens,
            temperature: None,
            thinking_budget: None,
        };

        let mut stream = match self.open_stream(request, tx, cancel).await {
            Ok(stream) => stream,
            Err(AppError::Cancelled) => {
                let _ = tx
                    .send(AgentEvent::CompactEnd(CompactOutcome::Cancelled))
                    .await;
                return Ok(CompactOutcome::Cancelled);
            }
            Err(error) => return Err(error),
        };

        let mut summary = String::new();
        let mut stop_reason = StopReason::EndTurn;
        while let Some(item) = stream.next().await {
            match item {
                Ok(ProviderEvent::TextDelta(text)) => summary.push_str(&text),
                Ok(ProviderEvent::Usage { input, output }) => {
                    let _ = tx.send(AgentEvent::Usage { input, output }).await;
                }
                Ok(ProviderEvent::MessageStop {
                    stop_reason: reason,
                }) => stop_reason = reason,
                Ok(_) => {}
                Err(error) => {
                    let _ = tx.send(AgentEvent::Error(error.to_string())).await;
                    return Err(error.into());
                }
            }
        }

        if cancel.is_cancelled() {
            let _ = tx
                .send(AgentEvent::CompactEnd(CompactOutcome::Cancelled))
                .await;
            return Ok(CompactOutcome::Cancelled);
        }

        let summary = summary.trim();
        if summary.is_empty() {
            let message = "the model returned an empty summary";
            let _ = tx.send(AgentEvent::Error(message.to_string())).await;
            return Err(AppError::runtime(message));
        }

        if let Some(session) = &self.session {
            let mut session = session.lock().await;
            if let Err(error) = session.append_compact(summary) {
                let message = format!("cannot save compaction: {error}");
                let _ = tx.send(AgentEvent::Error(message.clone())).await;
                return Err(AppError::runtime(message));
            }
        }

        self.messages = vec![crate::compact::summary_message(summary)];
        let report = CompactReport {
            messages_before,
            messages_after: self.messages.len(),
            tokens_before,
            tokens_after: estimate_tokens(&self.messages),
            summary_chars: summary.chars().count(),
            truncated: stop_reason == StopReason::MaxTokens,
        };
        let outcome = CompactOutcome::Compacted(report);
        let _ = tx.send(AgentEvent::CompactEnd(outcome.clone())).await;
        Ok(outcome)
    }

    async fn record(&mut self, message: Message) {
        if let Some(session) = &self.session {
            let mut session = session.lock().await;
            let _ = session.append(&message);
        }
        self.messages.push(message);
    }

    async fn execute_tools(
        &self,
        invocations: &[ToolInvocation],
        tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Vec<ToolResult> {
        let mut results: Vec<Option<ToolResult>> = vec![None; invocations.len()];
        let mut runnable: Vec<usize> = Vec::new();

        for (index, invocation) in invocations.iter().enumerate() {
            let summary = self
                .tools
                .get(&invocation.name)
                .map(|tool| match &invocation.arguments {
                    Ok(args) => tool.summary(args),
                    Err(_) => invocation.name.clone(),
                })
                .unwrap_or_else(|| invocation.name.clone());

            let _ = tx
                .send(AgentEvent::ToolCall {
                    id: invocation.id.clone(),
                    name: invocation.name.clone(),
                    summary: summary.clone(),
                })
                .await;

            let Some(tool) = self.tools.get(&invocation.name) else {
                let result = error_result(
                    invocation,
                    format!("unknown tool '{}'", invocation.name),
                );
                let _ = tx
                    .send(AgentEvent::ToolEnd {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                        content: result.content.clone(),
                        is_error: true,
                    })
                    .await;
                results[index] = Some(result);
                continue;
            };

            let args = match &invocation.arguments {
                Ok(args) => args.clone(),
                Err(error) => {
                    let result = error_result(invocation, error.clone());
                    let _ = tx
                        .send(AgentEvent::ToolEnd {
                            id: invocation.id.clone(),
                            name: invocation.name.clone(),
                            content: result.content.clone(),
                            is_error: true,
                        })
                        .await;
                    results[index] = Some(result);
                    continue;
                }
            };

            let path = tool
                .path_arg(&args)
                .map(|raw| crate::tools::resolve_path(&self.cwd, &raw));
            let detail = permission_detail(&invocation.name, &args);
            let request = crate::permissions::request_for(
                &invocation.name,
                summary.clone(),
                detail,
                &args,
                &self.cwd,
            );
            let decision = self
                .permissions
                .check(&request, path.as_deref(), self.prompter.as_ref())
                .await;

            if !decision.is_allowed() {
                let result = error_result(
                    invocation,
                    format!(
                        "permission denied for {} (mode: {})",
                        invocation.name,
                        self.permissions.mode.label()
                    ),
                );
                let _ = tx
                    .send(AgentEvent::ToolEnd {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                        content: result.content.clone(),
                        is_error: true,
                    })
                    .await;
                results[index] = Some(result);
                continue;
            }

            runnable.push(index);
        }

        let ctx = ToolCtx {
            cwd: self.cwd.clone(),
            cancel: cancel.clone(),
            todos: self.todos.clone(),
        };

        let mut cursor = 0;
        while cursor < runnable.len() {
            let index = runnable[cursor];
            let invocation = &invocations[index];
            let Some(tool) = self.tools.get(&invocation.name) else {
                cursor += 1;
                continue;
            };
            let args = invocation
                .arguments
                .clone()
                .unwrap_or(Value::Object(Default::default()));

            if tool.execution_mode() == ExecutionMode::Parallel {
                let mut batch = vec![index];
                let mut next = cursor + 1;
                while next < runnable.len() {
                    let candidate = &invocations[runnable[next]];
                    match self.tools.get(&candidate.name) {
                        Some(candidate_tool)
                            if candidate_tool.execution_mode() == ExecutionMode::Parallel =>
                        {
                            batch.push(runnable[next]);
                            next += 1;
                        }
                        _ => break,
                    }
                }

                for &item in &batch {
                    let _ = tx
                        .send(AgentEvent::ToolStart {
                            id: invocations[item].id.clone(),
                            name: invocations[item].name.clone(),
                        })
                        .await;
                }

                let futures = batch.iter().map(|&item| {
                    let tool = self
                        .tools
                        .get(&invocations[item].name)
                        .expect("tool checked above");
                    let args = invocations[item]
                        .arguments
                        .clone()
                        .unwrap_or(Value::Object(Default::default()));
                    let ctx = ctx.clone();
                    async move { tool.execute(&args, &ctx).await }
                });
                let outputs = futures_util::future::join_all(futures).await;

                for (&item, output) in batch.iter().zip(outputs) {
                    let _ = tx
                        .send(AgentEvent::ToolEnd {
                            id: invocations[item].id.clone(),
                            name: invocations[item].name.clone(),
                            content: output.content.clone(),
                            is_error: output.is_error,
                        })
                        .await;
                    results[item] = Some(ToolResult {
                        tool_call_id: invocations[item].id.clone(),
                        tool_name: invocations[item].name.clone(),
                        content: output.content,
                        is_error: output.is_error,
                    });
                }

                cursor = next;
            } else {
                let _ = tx
                    .send(AgentEvent::ToolStart {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                    })
                    .await;
                let output = tool.execute(&args, &ctx).await;
                let _ = tx
                    .send(AgentEvent::ToolEnd {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                        content: output.content.clone(),
                        is_error: output.is_error,
                    })
                    .await;
                results[index] = Some(ToolResult {
                    tool_call_id: invocation.id.clone(),
                    tool_name: invocation.name.clone(),
                    content: output.content,
                    is_error: output.is_error,
                });
                cursor += 1;
            }
        }

        if invocations.iter().any(|invocation| invocation.name == "todowrite") {
            let todos = self.todos.lock().await.clone();
            let _ = tx.send(AgentEvent::Todos(todos)).await;
        }

        results
            .into_iter()
            .map(|result| {
                result.unwrap_or_else(|| ToolResult {
                    tool_call_id: "unknown".into(),
                    tool_name: "unknown".into(),
                    content: "tool was not executed".into(),
                    is_error: true,
                })
            })
            .collect()
    }
}

fn error_result(invocation: &ToolInvocation, message: String) -> ToolResult {
    ToolResult {
        tool_call_id: invocation.id.clone(),
        tool_name: invocation.name.clone(),
        content: message,
        is_error: true,
    }
}

fn permission_detail(tool: &str, args: &Value) -> String {
    match tool {
        "bash" => args
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "write" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("?");
            let size = args
                .get("content")
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or(0);
            format!("write {path} ({size} bytes)")
        }
        "edit" => {
            let path = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            format!("edit {path}")
        }
        "read" | "grep" | "glob" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            format!("{tool} {path}")
        }
        _ => tool.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionMode;
    use crate::permissions::DenyPrompter;
    use crate::providers::mock::MockProvider;
    use crate::providers::{ProviderError, ProviderEvent};
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
}
