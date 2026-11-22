use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::events::{AgentEvent, CompactOutcome, CompactReport};
use crate::error::{AppError, Result};
use crate::protocol::{StopReason, estimate_tokens};
use crate::providers::{CompletionRequest, ProviderEvent};

use super::Agent;

impl Agent {
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
}
