use std::sync::Arc;

use tokio::sync::mpsc;

use crate::agent::{Agent, AgentEvent, AgentOptions};
use crate::tools::ToolRegistry;

use super::store::MemberStatus;
use super::tools::register_member;
use super::worktree::IsolatedWorktree;
use super::Team;

const MAX_STEPS: usize = 32;
const MAX_REPORT_BYTES: usize = 16 * 1024;

/// What the lead asks a background teammate to do.
pub struct TeammateRequest {
    pub name: String,
    pub role: String,
    pub prompt: String,
    /// Optional tool whitelist; team tools are always added.
    pub tools: Option<Vec<String>>,
    /// When true, run the teammate in an isolated detached git worktree.
    pub isolate: bool,
}

impl Team {
    /// Register a teammate and start it in the background. Returns the
    /// sanitized name used on the task board and in messages.
    pub async fn spawn(self: &Arc<Self>, request: TeammateRequest) -> Result<String, String> {
        let name = sanitize_name(&request.name);
        if name.is_empty() {
            return Err("teammate name is empty".to_string());
        }
        if request.prompt.trim().is_empty() {
            return Err("teammate prompt is empty".to_string());
        }
        let role = if request.role.trim().is_empty() {
            "teammate".to_string()
        } else {
            request.role.trim().to_string()
        };

        self.register_member(&name, &role).await?;
        self.publish().await;

        let team = self.clone();
        let request = TeammateRequest {
            name: name.clone(),
            role,
            ..request
        };
        tokio::spawn(async move { run(team, request).await });
        Ok(name)
    }
}

async fn run(team: Arc<Team>, request: TeammateRequest) {
    let worktree = if request.isolate {
        IsolatedWorktree::create(&team.runtime.cwd, &request.name).ok()
    } else {
        None
    };
    let cwd = worktree
        .as_ref()
        .map(|wt| wt.path.clone())
        .unwrap_or_else(|| team.runtime.cwd.clone());
    let permissions = if worktree.is_some() {
        Arc::new(crate::permissions::PermissionEngine::with_sandbox(
            team.runtime.permissions.mode(),
            team.runtime.permissions.sandbox(),
            cwd.clone(),
            Vec::new(),
        ))
    } else {
        team.runtime.permissions.clone()
    };

    let mut system = team.runtime.base_system.clone();
    system.push(rules(&request.name, &request.role));
    if let Some(wt) = &worktree {
        system.push(format!(
            "<worktree>\nYou are running in an isolated git worktree at {}.\n\
             Edits here do not modify the lead's working copy.\n</worktree>",
            wt.path.display()
        ));
    }

    let mut registry = ToolRegistry::with_skills(team.runtime.skills.clone());
    if let Some(mcp) = &team.runtime.mcp {
        for tool in mcp.tools() {
            registry.register(tool);
        }
    }
    if let Some(allowed) = &request.tools {
        registry.retain(allowed);
    }
    register_member(&mut registry, team.clone(), &request.name);

    let mut agent = Agent::new(AgentOptions {
        provider: team.runtime.provider.clone(),
        tools: Arc::new(registry),
        permissions,
        prompter: team.runtime.prompter.clone(),
        system,
        cwd,
        max_tokens: team.runtime.max_tokens,
        temperature: team.runtime.temperature,
        thinking_budget: team.runtime.thinking_budget,
        max_steps: Some(MAX_STEPS),
        session: None,
        messages: Vec::new(),
        retry: team.runtime.retry,
    });

    let (tx, rx) = mpsc::channel(256);
    let collector = tokio::spawn(collect(rx, team.clone(), request.name.clone()));
    let cancel = team.cancellation();
    let result = agent.run_turn(&request.prompt, &tx, &cancel).await;
    drop(tx);
    let (text, trace) = collector.await.unwrap_or_default();

    let status = if result.is_ok() {
        MemberStatus::Done
    } else {
        MemberStatus::Failed
    };
    let mut report = format!(
        "<teammate name=\"{}\" status=\"{}\">\n",
        request.name,
        status.label()
    );
    if let Err(error) = &result {
        report.push_str(&format!("error: {error}\n"));
    }
    if text.trim().is_empty() {
        report.push_str("(the teammate returned no text)\n");
    } else {
        report.push_str(text.trim());
        report.push('\n');
    }
    if let Some(wt) = worktree {
        if let Some(changes) = wt.changes_summary() {
            report.push_str(&format!("\n{changes}\n"));
        } else {
            wt.remove();
        }
    }
    if !trace.is_empty() {
        report.push_str("\ntools:\n");
        for item in trace.iter().take(20) {
            report.push_str(&format!("- {item}\n"));
        }
    }
    report.push_str("</teammate>");
    let report = cap(report);

    team.finish_member(&request.name, status, &report).await;
    let _ = team.send_message(&request.name, "lead", &report).await;
    team.publish().await;
}

fn rules(name: &str, role: &str) -> String {
    format!(
        "<teammate name=\"{name}\" role=\"{role}\">\n\
         You are a teammate in a multi-agent team. The lead agent owns the plan and talks to the user.\n\
         - Work only on the assignment in this message; do not broaden the scope.\n\
         - The shared task board is the source of truth: mark your task in_progress when you start and completed when you finish, with owner \"{name}\".\n\
         - Send a concise report to the lead with send_message (to: \"lead\") before you stop: what you did, what you found, and what you verified.\n\
         - Use check_messages when you are waiting for instructions from the lead.\n\
         - You cannot ask the user questions and you cannot spawn other teammates. If you are blocked, report it to the lead.\n\
         </teammate>"
    )
}

async fn collect(
    mut rx: mpsc::Receiver<AgentEvent>,
    team: Arc<Team>,
    name: String,
) -> (String, Vec<String>) {
    let mut current = String::new();
    let mut last_turn = String::new();
    let mut trace = Vec::new();

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::TurnStart { .. } => current.clear(),
            AgentEvent::TextDelta(text) => current.push_str(&text),
            AgentEvent::TurnEnd { .. } => {
                if !current.trim().is_empty() {
                    last_turn = current.clone();
                }
                current.clear();
            }
            AgentEvent::ToolCall {
                name: tool,
                summary,
                ..
            } => {
                trace.push(format!("{tool}: {summary}"));
                team.set_activity(&name, Some(format!("{tool} {summary}")))
                    .await;
                team.publish().await;
            }
            AgentEvent::Error(error) => trace.push(format!("error: {error}")),
            _ => {}
        }
    }

    (last_turn, trace)
}

/// Lowercase ASCII slug; names are used in prompts and message routing.
pub(super) fn sanitize_name(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | ' ' | '.') && !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn cap(mut text: String) -> String {
    if text.len() <= MAX_REPORT_BYTES {
        return text;
    }
    let mut end = MAX_REPORT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[teammate report truncated]");
    text
}

#[cfg(test)]
mod tests {
    use super::sanitize_name;

    #[test]
    fn names_become_ascii_slugs() {
        assert_eq!(sanitize_name("  Parser Pro "), "parser-pro");
        assert_eq!(sanitize_name("a__b"), "a-b");
        assert_eq!(sanitize_name("!!!"), "");
    }
}
