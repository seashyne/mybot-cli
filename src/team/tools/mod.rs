mod message;
mod spawn;
mod task;

use std::sync::Arc;

use crate::tools::ToolRegistry;

use super::Team;

/// Tools that let the lead agent run a team.
pub fn register_lead(registry: &mut ToolRegistry, team: Arc<Team>) {
    registry.register(Arc::new(task::TaskCreate::new(team.clone())));
    registry.register(Arc::new(task::TaskList::new(team.clone())));
    registry.register(Arc::new(task::TaskUpdate::new(team.clone(), "lead")));
    registry.register(Arc::new(message::SendMessage::new(team.clone(), "lead")));
    registry.register(Arc::new(message::CheckMessages::new(team.clone(), "lead")));
    registry.register(Arc::new(spawn::SpawnTeammate::new(team)));
}

/// Tools available inside a background teammate.
pub fn register_member(registry: &mut ToolRegistry, team: Arc<Team>, name: &str) {
    registry.register(Arc::new(task::TaskList::new(team.clone())));
    registry.register(Arc::new(task::TaskUpdate::new(team.clone(), name)));
    registry.register(Arc::new(message::SendMessage::new(team.clone(), name)));
    registry.register(Arc::new(message::CheckMessages::new(team, name)));
}

pub(super) const MAX_OUTPUT_BYTES: usize = 32 * 1024;

pub(super) fn cap(mut text: String) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text;
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[team output truncated]");
    text
}
