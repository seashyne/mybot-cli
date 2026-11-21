use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{CompletionRequest, EventStream, Provider, ProviderError, ProviderEvent};
use crate::protocol::StopReason;

/// Scripted provider used by tests and demos: each call to `stream` returns the
/// next scripted event list. Queued failures are returned first, one per call.
pub struct MockProvider {
    name: String,
    model: String,
    scripts: Mutex<VecDeque<Vec<ProviderEvent>>>,
    failures: Mutex<VecDeque<ProviderError>>,
    calls: AtomicUsize,
}

impl MockProvider {
    pub fn new(scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            name: "mock".into(),
            model: "mock-model".into(),
            scripts: Mutex::new(scripts.into()),
            failures: Mutex::new(VecDeque::new()),
            calls: AtomicUsize::new(0),
        }
    }

    /// Fail the first `failures.len()` calls, then serve `scripts`.
    pub fn with_failures(failures: Vec<ProviderError>, scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            failures: Mutex::new(failures.into()),
            ..Self::new(scripts)
        }
    }

    pub fn simple(text: impl Into<String>) -> Self {
        Self::new(vec![vec![
            ProviderEvent::TextDelta(text.into()),
            ProviderEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            },
        ]])
    }

    /// How many times `stream` was called (successful or not).
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn stream(
        &self,
        _req: CompletionRequest,
        _cancel: CancellationToken,
    ) -> Result<EventStream, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.failures.lock().expect("mock provider lock").pop_front() {
            return Err(error);
        }
        let events = self
            .scripts
            .lock()
            .expect("mock provider lock")
            .pop_front()
            .unwrap_or_else(|| {
                vec![ProviderEvent::MessageStop {
                    stop_reason: StopReason::EndTurn,
                }]
            });
        let stream = futures_util::stream::iter(events.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }
}
