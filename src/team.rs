//! In-session team: a shared task board, a message bus, and background
//! teammates that run their own agent loop with the same tools and provider.

mod runner;
mod store;
mod tools;
#[cfg(test)]
mod tests;

pub use store::{Member, MemberStatus, TeamMessage, TeamSnapshot, TeamTask};
pub use tools::register_lead;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::agent::AgentEvent;
use crate::subagents::AgentRuntime;

pub struct Team {
    tasks: Mutex<Vec<TeamTask>>,
    members: Mutex<Vec<Member>>,
    lead_inbox: Mutex<Vec<TeamMessage>>,
    member_inboxes: Mutex<HashMap<String, Vec<TeamMessage>>>,
    next_task: AtomicU64,
    events: mpsc::Sender<AgentEvent>,
    cancel: CancellationToken,
    pub runtime: Arc<AgentRuntime>,
}

impl Team {
    pub fn new(runtime: Arc<AgentRuntime>, events: mpsc::Sender<AgentEvent>) -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            members: Mutex::new(Vec::new()),
            lead_inbox: Mutex::new(Vec::new()),
            member_inboxes: Mutex::new(HashMap::new()),
            next_task: AtomicU64::new(1),
            events,
            cancel: CancellationToken::new(),
            runtime,
        }
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

/// Compact rendering used by the `/team` command.
pub fn render_snapshot(snapshot: &TeamSnapshot) -> String {
    let mut text = String::from("team\n");
    if snapshot.tasks.is_empty() {
        text.push_str("  tasks: none\n");
    } else {
        text.push_str(&format!("  tasks ({}):\n", snapshot.tasks.len()));
        for task in &snapshot.tasks {
            let owner = task
                .owner
                .as_deref()
                .map(|owner| format!(" @{owner}"))
                .unwrap_or_default();
            text.push_str(&format!(
                "    [{}] {} — {}{owner}\n",
                task.status, task.id, task.subject
            ));
        }
    }
    if snapshot.members.is_empty() {
        text.push_str("  members: none");
    } else {
        text.push_str(&format!(
            "  members ({}, {} running):\n",
            snapshot.members.len(),
            snapshot.running()
        ));
        for member in &snapshot.members {
            text.push_str(&format!(
                "    {:<14} {:<12} {}\n",
                member.name,
                member.role,
                member.status.label()
            ));
        }
        text.truncate(text.trim_end().len());
    }
    text
}
