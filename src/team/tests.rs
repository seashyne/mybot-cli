use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use super::runner::TeammateRequest;
use super::tools::register_member;
use super::*;
use crate::agent::AgentEvent;
use crate::config::PermissionMode;
use crate::permissions::{DenyPrompter, PermissionEngine};
use crate::providers::Provider;
use crate::providers::RetryPolicy;
use crate::providers::mock::MockProvider;
use crate::skills::SkillSet;
use crate::subagents::{AgentRuntime, AgentSet};
use crate::tools::{ToolCtx, ToolRegistry};

fn runtime(provider: Arc<dyn Provider>, cwd: PathBuf) -> AgentRuntime {
    AgentRuntime {
        provider,
        permissions: Arc::new(PermissionEngine::new(
            PermissionMode::Auto,
            cwd.clone(),
            Vec::new(),
        )),
        prompter: Arc::new(DenyPrompter),
        base_system: vec!["base".into()],
        cwd,
        max_tokens: 100,
        temperature: None,
        thinking_budget: None,
        retry: RetryPolicy::default(),
        skills: Arc::new(SkillSet::default()),
        mcp: None,
        agents: Arc::new(AgentSet::default()),
    }
}

fn make(provider: Arc<dyn Provider>) -> (Arc<Team>, mpsc::Receiver<AgentEvent>) {
    let (tx, rx) = mpsc::channel(64);
    let team = Arc::new(Team::new(
        Arc::new(runtime(provider, std::env::temp_dir())),
        tx,
    ));
    (team, rx)
}

fn ctx() -> ToolCtx {
    ToolCtx {
        cwd: std::env::temp_dir(),
        cancel: CancellationToken::new(),
        todos: Arc::new(Mutex::new(Vec::new())),
        checkpoints: crate::tools::checkpoint::CheckpointStore::new(),
        sandbox: crate::config::SandboxMode::Auto,
    }
}

#[tokio::test]
async fn task_board_tracks_status_and_owner() {
    let (team, _rx) = make(Arc::new(MockProvider::simple("unused")));
    let task = team.create_task("fix parser", "make the tests pass").await;
    assert_eq!(task.id, "t1");
    assert_eq!(task.status, "pending");

    let updated = team
        .update_task("t1", Some("in_progress"), Some("alice"), None)
        .await
        .unwrap();
    assert_eq!(updated.status, "in_progress");
    assert_eq!(updated.owner.as_deref(), Some("alice"));

    assert!(team.update_task("t1", Some("nope"), None, None).await.is_err());
    assert!(team.update_task("t9", None, None, None).await.is_err());
}

#[tokio::test]
async fn teammate_runs_in_background_and_reports_to_the_lead() {
    let provider = Arc::new(MockProvider::simple("fixed the parser"));
    let (team, _rx) = make(provider);

    let name = team
        .spawn(TeammateRequest {
            name: "Parser Pro".into(),
            role: "engineer".into(),
            prompt: "fix the parser".into(),
            tools: None,
            isolate: false,
        })
        .await
        .unwrap();
    assert_eq!(name, "parser-pro");
    assert!(team.wait_idle(Duration::from_secs(5)).await);

    let messages = team.drain_inbox("lead").await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "parser-pro");
    assert!(messages[0].content.contains("fixed the parser"));
    assert!(messages[0].content.contains("status=\"done\""));

    let members = team.members().await;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].status, MemberStatus::Done);
    assert!(members[0].activity.is_none());
}

#[tokio::test]
async fn duplicate_teammate_names_are_rejected() {
    let (team, _rx) = make(Arc::new(MockProvider::simple("unused")));
    team.register_member("alice", "reviewer").await.unwrap();
    let error = team
        .spawn(TeammateRequest {
            name: "Alice".into(),
            role: String::new(),
            prompt: "review".into(),
            tools: None,
            isolate: false,
        })
        .await
        .unwrap_err();
    assert!(error.contains("already exists"));
}

#[tokio::test]
async fn messages_route_to_members_and_the_lead() {
    let (team, _rx) = make(Arc::new(MockProvider::simple("unused")));
    team.register_member("bob", "tester").await.unwrap();

    team.send_message("lead", "bob", "check the tests").await.unwrap();
    let inbox = team.drain_inbox("bob").await;
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].content, "check the tests");
    assert!(team.drain_inbox("bob").await.is_empty());

    assert!(team.send_message("lead", "nobody", "hi").await.is_err());
    assert!(team.send_message("lead", "bob", "   ").await.is_err());

    team.send_message("bob", "lead", "done").await.unwrap();
    let inbox = team.drain_inbox("lead").await;
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].from, "bob");
}

#[tokio::test]
async fn member_task_update_claims_the_task() {
    let (team, _rx) = make(Arc::new(MockProvider::simple("unused")));
    let task = team.create_task("write tests", "").await;

    let mut registry = ToolRegistry::new();
    register_member(&mut registry, team.clone(), "alice");
    let tool = registry.get("task_update").unwrap();
    let output = tool
        .execute(
            &json!({"id": task.id, "status": "in_progress"}),
            &ctx(),
        )
        .await;

    assert!(!output.is_error, "{}", output.content);
    let tasks = team.tasks().await;
    assert_eq!(tasks[0].status, "in_progress");
    assert_eq!(tasks[0].owner.as_deref(), Some("alice"));
}

#[tokio::test]
async fn lead_registry_exposes_spawning() {
    let (team, _rx) = make(Arc::new(MockProvider::simple("unused")));
    let mut registry = ToolRegistry::new();
    register_lead(&mut registry, team);
    for name in ["task_create", "task_list", "task_update", "send_message", "check_messages", "spawn_teammate"] {
        assert!(registry.get(name).is_some(), "missing {name}");
    }
}

#[tokio::test]
async fn snapshot_renders_tasks_and_members() {
    let (team, _rx) = make(Arc::new(MockProvider::simple("unused")));
    team.create_task("fix parser", "").await;
    team.register_member("alice", "engineer").await.unwrap();
    team.update_task("t1", Some("completed"), Some("alice"), None)
        .await
        .unwrap();
    team.finish_member("alice", MemberStatus::Done, "ok")
        .await;

    let text = render_snapshot(&team.snapshot().await);
    assert!(text.contains("tasks (1)"));
    assert!(text.contains("[completed] t1 — fix parser @alice"));
    assert!(text.contains("members (1, 0 running)"));
    assert!(text.contains("alice"));
}
