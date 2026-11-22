use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::config::PermissionMode;
use crate::permissions::{DenyPrompter, PermissionEngine};
use crate::providers::RetryPolicy;
use crate::providers::mock::MockProvider;
use crate::skills::SkillSet;
use crate::tools::{Tool, ToolCtx};

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn definition(name: &str, tools: Option<&str>) -> String {
    match tools {
        Some(tools) => format!(
            "---\nname: {name}\ndescription: {name} helper\ntools: {tools}\n---\nYou are {name}.\n"
        ),
        None => format!("---\nname: {name}\ndescription: {name} helper\n---\nYou are {name}.\n"),
    }
}

#[test]
fn parses_definition_with_tool_subset() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("explore.md"),
        &definition("explore", Some("read, grep, glob")),
    );

    let set = AgentSet::from_roots(&[dir.path().to_path_buf()]);
    let agent = set.get("explore").unwrap();
    assert_eq!(agent.description, "explore helper");
    assert_eq!(
        agent.tools.as_deref(),
        Some(&["read".to_string(), "grep".into(), "glob".into()][..])
    );
    assert!(agent.body.contains("You are explore."));
}

#[test]
fn project_definitions_override_user_ones() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user");
    let project = dir.path().join("project");
    write(&user.join("review.md"), &definition("review", None));
    write(
        &project.join("review.md"),
        "---\nname: review\ndescription: project review\n---\nproject body\n",
    );

    let set = AgentSet::from_roots(&[user, project]);
    assert_eq!(set.agents.len(), 1);
    assert_eq!(set.get("review").unwrap().description, "project review");
}

#[test]
fn prompt_block_lists_agents() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("a.md"), &definition("a", None));
    write(&dir.path().join("b.md"), &definition("b", None));
    let set = AgentSet::from_roots(&[dir.path().to_path_buf()]);
    let block = set.prompt_block().unwrap();
    assert!(block.contains("<subagents>"));
    assert!(block.contains("- a: a helper"));
    assert!(block.contains("- b: b helper"));
}

fn runtime(provider: Arc<dyn crate::providers::Provider>, cwd: PathBuf) -> AgentRuntime {
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

fn ctx(cwd: &std::path::Path) -> ToolCtx {
    ToolCtx {
        cwd: cwd.to_path_buf(),
        cancel: CancellationToken::new(),
        todos: Arc::new(Mutex::new(Vec::new())),
        checkpoints: crate::tools::checkpoint::CheckpointStore::new(),
        sandbox: crate::config::SandboxMode::Auto,
    }
}

#[tokio::test]
async fn runs_a_subagent_and_returns_its_report() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("found three call sites"));
    let tool = AgentTool::new(Arc::new(runtime(provider, dir.path().to_path_buf())));

    let output = tool
        .execute(
            &json!({"description": "scan call sites", "prompt": "find call sites"}),
            &ctx(dir.path()),
        )
        .await;

    assert!(!output.is_error, "{}", output.content);
    assert!(output.content.contains("<subagent name=\"general\""));
    assert!(output.content.contains("found three call sites"));
}

#[tokio::test]
async fn rejects_unknown_subagent_type() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("unused"));
    let tool = AgentTool::new(Arc::new(runtime(provider, dir.path().to_path_buf())));

    let output = tool
        .execute(
            &json!({"description": "x", "prompt": "y", "subagent_type": "nope"}),
            &ctx(dir.path()),
        )
        .await;
    assert!(output.is_error);
    assert!(output.content.contains("unknown subagent_type"));
}

#[tokio::test]
async fn requires_a_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("unused"));
    let tool = AgentTool::new(Arc::new(runtime(provider, dir.path().to_path_buf())));

    let output = tool
        .execute(&json!({"description": "x"}), &ctx(dir.path()))
        .await;
    assert!(output.is_error);
    assert!(output.content.contains("non-empty prompt"));
}

#[tokio::test]
async fn summary_names_the_agent_type() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::simple("unused"));
    let tool = AgentTool::new(Arc::new(runtime(provider, dir.path().to_path_buf())));
    let summary = tool.summary(&json!({"description": "scan", "subagent_type": "explore"}));
    assert_eq!(summary, "agent explore · scan");
}
