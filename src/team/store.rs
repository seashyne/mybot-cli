use std::sync::atomic::Ordering;

use serde::Serialize;

use super::Team;

#[derive(Debug, Clone, Serialize)]
pub struct TeamTask {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: String,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberStatus {
    Running,
    Done,
    Failed,
}

impl MemberStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Member {
    pub name: String,
    pub role: String,
    pub status: MemberStatus,
    /// Latest tool activity while running, e.g. "read src/parser.rs".
    pub activity: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamMessage {
    pub from: String,
    pub to: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TeamSnapshot {
    pub tasks: Vec<TeamTask>,
    pub members: Vec<Member>,
}

impl TeamSnapshot {
    pub fn running(&self) -> usize {
        self.members
            .iter()
            .filter(|member| member.status == MemberStatus::Running)
            .count()
    }
}

impl Team {
    pub async fn create_task(&self, subject: &str, description: &str) -> TeamTask {
        let id = format!("t{}", self.next_task.fetch_add(1, Ordering::SeqCst));
        let task = TeamTask {
            id,
            subject: cap(subject.trim(), 200),
            description: cap(description.trim(), 4000),
            status: "pending".to_string(),
            owner: None,
        };
        self.tasks.lock().await.push(task.clone());
        self.publish().await;
        task
    }

    pub async fn update_task(
        &self,
        id: &str,
        status: Option<&str>,
        owner: Option<&str>,
        subject: Option<&str>,
    ) -> Result<TeamTask, String> {
        if let Some(status) = status
            && !matches!(status, "pending" | "in_progress" | "completed") {
                return Err(format!(
                    "invalid status '{status}' (expected pending, in_progress, or completed)"
                ));
            }

        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .iter_mut()
            .find(|task| task.id == id)
            .ok_or_else(|| format!("unknown task '{id}'"))?;
        if let Some(status) = status {
            task.status = status.to_string();
        }
        if let Some(owner) = owner {
            task.owner = if owner.trim().is_empty() {
                None
            } else {
                Some(owner.trim().to_string())
            };
        }
        if let Some(subject) = subject
            && !subject.trim().is_empty() {
                task.subject = subject.trim().to_string();
            }
        let updated = task.clone();
        drop(tasks);
        self.publish().await;
        Ok(updated)
    }

    pub async fn tasks(&self) -> Vec<TeamTask> {
        self.tasks.lock().await.clone()
    }

    pub async fn members(&self) -> Vec<Member> {
        self.members.lock().await.clone()
    }

    pub async fn snapshot(&self) -> TeamSnapshot {
        TeamSnapshot {
            tasks: self.tasks().await,
            members: self.members().await,
        }
    }

    pub async fn send_message(&self, from: &str, to: &str, content: &str) -> Result<(), String> {
        let content = content.trim();
        if content.is_empty() {
            return Err("message content is empty".to_string());
        }
        let to = to.trim();
        if to.is_empty() {
            return Err("message recipient is empty".to_string());
        }

        let message = TeamMessage {
            from: from.to_string(),
            to: to.to_string(),
            content: content.to_string(),
        };

        if matches!(to, "lead" | "main" | "user") {
            self.lead_inbox.lock().await.push(message);
            return Ok(());
        }

        let members = self.members.lock().await;
        if !members.iter().any(|member| member.name == to) {
            return Err(format!(
                "unknown recipient '{to}' (members: {})",
                members
                    .iter()
                    .map(|member| member.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        drop(members);

        self.member_inboxes
            .lock()
            .await
            .entry(to.to_string())
            .or_default()
            .push(message);
        Ok(())
    }

    pub async fn drain_inbox(&self, identity: &str) -> Vec<TeamMessage> {
        if matches!(identity, "lead" | "main") {
            return std::mem::take(&mut *self.lead_inbox.lock().await);
        }
        self.member_inboxes
            .lock()
            .await
            .remove(identity)
            .unwrap_or_default()
    }

    pub async fn publish(&self) {
        let snapshot = self.snapshot().await;
        // Never block the agent loop on UI delivery: a dropped snapshot is
        // replaced by the next one.
        let _ = self
            .events
            .try_send(crate::agent::AgentEvent::Team(snapshot));
    }

    /// Wait until every teammate stopped, or the timeout expires.
    pub async fn wait_idle(&self, timeout: std::time::Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self
                .members
                .lock()
                .await
                .iter()
                .all(|member| member.status != MemberStatus::Running)
            {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    pub async fn register_member(&self, name: &str, role: &str) -> Result<(), String> {
        let mut members = self.members.lock().await;
        if members.iter().any(|member| member.name == name) {
            return Err(format!("teammate '{name}' already exists"));
        }
        members.push(Member {
            name: name.to_string(),
            role: role.to_string(),
            status: MemberStatus::Running,
            activity: None,
            result: None,
        });
        Ok(())
    }

    pub async fn set_activity(&self, name: &str, activity: Option<String>) {
        let mut members = self.members.lock().await;
        if let Some(member) = members.iter_mut().find(|member| member.name == name) {
            member.activity = activity.map(|activity| cap(&activity, 120));
        }
    }

    pub async fn finish_member(&self, name: &str, status: MemberStatus, result: &str) {
        let mut members = self.members.lock().await;
        if let Some(member) = members.iter_mut().find(|member| member.name == name) {
            member.status = status;
            member.activity = None;
            member.result = Some(cap(result, 2000));
        }
    }
}

fn cap(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "…"
}
