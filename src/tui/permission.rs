use tokio::sync::oneshot;

use crate::permissions::{Decision, PermissionRequest};

/// A pending permission question shown as a modal over the transcript.
pub struct PermissionModal {
    pub request: PermissionRequest,
    pub reply: oneshot::Sender<Decision>,
}

impl PermissionModal {
    pub fn new(request: PermissionRequest, reply: oneshot::Sender<Decision>) -> Self {
        Self { request, reply }
    }

    pub fn answer(self, decision: Decision) {
        let _ = self.reply.send(decision);
    }
}
