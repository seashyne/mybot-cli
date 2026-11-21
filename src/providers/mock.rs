use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{CompletionRequest, EventStream, Provider, ProviderError, ProviderEvent};
use crate::protocol::StopReason;

/// Scripted provider used by tests and demos: each call to `stream` returns the
/// next scripted event list.
pub struct MockProvider {
    name: String,
    model: String,
    scripts: Mutex<VecDeque<Vec<ProviderEvent>>>,
}

impl MockProvider {
    pub fn new(scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            name: "mock".into(),
            model: "mock-model".into(),
            scripts: Mutex::new(scripts.into()),
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
