use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, Result};
use crate::permissions::{PermissionEngine, Prompter};
use crate::protocol::{ImageAttachment, Message, StopReason};
use crate::providers::{CompletionRequest, EventStream, Provider, ProviderEvent, RetryPolicy};
use crate::session::Session;
use crate::tools::checkpoint::CheckpointStore;
use crate::tools::{TodoItem, ToolRegistry};

use super::assembler::Assembler;
use super::events::AgentEvent;

mod compact;
mod tools;
#[cfg(test)]
mod compact_tests;
#[cfg(test)]
mod tests;

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
    checkpoints: CheckpointStore,
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
            checkpoints: CheckpointStore::new(),
            retry: options.retry,
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn system(&self) -> &[String] {
        &self.system
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

    pub fn checkpoints(&self) -> CheckpointStore {
        self.checkpoints.clone()
    }

    /// Swap the model client at runtime without touching the conversation.
    pub fn set_provider(&mut self, options: ProviderSwap) {
        self.provider = options.provider;
        self.max_tokens = options.max_tokens;
        self.temperature = options.temperature;
        self.thinking_budget = options.thinking_budget;
        self.retry = options.retry;
    }

    pub fn set_thinking_budget(&mut self, budget: Option<u32>) {
        self.thinking_budget = budget;
    }

    /// Run one user turn to completion, including any tool-call iterations.
    pub async fn run_turn(
        &mut self,
        input: &str,
        tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<StopReason> {
        self.run_turn_with_images(input, Vec::new(), tx, cancel)
            .await
    }

    /// Run one user turn with optional attached images.
    pub async fn run_turn_with_images(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<StopReason> {
        self.checkpoints.begin_turn(input);
        self.record(Message::user_with_images(input, images)).await;

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
    pub(super) async fn open_stream(
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
        self.reset_with(session, Vec::new()).await;
    }

    /// Replace the conversation with a loaded session's history.
    pub async fn reset_with(
        &mut self,
        session: Option<Arc<Mutex<Session>>>,
        messages: Vec<Message>,
    ) {
        self.messages = messages;
        self.session = session;
        self.todos.lock().await.clear();
    }

    async fn record(&mut self, message: Message) {
        if let Some(session) = &self.session {
            let mut session = session.lock().await;
            let _ = session.append(&message);
        }
        self.messages.push(message);
    }
}
