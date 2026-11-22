use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentEvent, AgentOptions, CompactOutcome};
use crate::config::{Config, PermissionMode};
use crate::error::{AppError, Result};
use crate::permissions::{DenyPrompter, PermissionEngine, Prompter};
use crate::prompt::{self, PromptOptions};
use crate::providers;
use crate::session::Session;
use crate::tools::ToolRegistry;

pub struct ExecOptions {
    pub prompt: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub output_format: String,
    pub output_last_message: Option<PathBuf>,
    pub timeout_secs: u64,
    pub max_steps: Option<usize>,
    pub permission: PermissionMode,
    pub system_override: Option<String>,
    pub append_system: Vec<String>,
    pub session_id: Option<String>,
    pub continue_: bool,
}

pub async fn run(options: ExecOptions) -> Result<()> {
    let cwd = options
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| options.cwd.clone());

    let config = Config::load()?;
    let resolved = config.resolve(options.model.as_deref())?;
    let provider: Arc<dyn crate::providers::Provider> =
        Arc::from(providers::build(&resolved)?);

    let skills = Arc::new(crate::skills::SkillSet::discover(&cwd));
    let memory = crate::memory::MemoryStore::load(&cwd);
    let mcp = Arc::new(crate::mcp::McpManager::start(&cwd).await);
    for warning in mcp.warnings() {
        eprintln!("mybot: mcp: {warning}");
    }
    let agents = Arc::new(crate::subagents::AgentSet::discover(&cwd));
    let system = prompt::build(&PromptOptions {
        cwd: cwd.clone(),
        system_override: options.system_override.clone(),
        append: options.append_system.clone(),
        skills: skills.prompt_block(),
        memory: memory.prompt_block(),
        agents: agents.prompt_block(),
    });
    let mut registry = ToolRegistry::with_skills(skills.clone());
    for tool in mcp.tools() {
        registry.register(tool);
    }

    let session = open_session(&options, &cwd, &resolved.name, &resolved.model)?;
    let messages = session.messages.clone();
    let session = Arc::new(Mutex::new(session));

    let permissions = Arc::new(PermissionEngine::with_sandbox(
        options.permission,
        config.permissions.sandbox,
        cwd.clone(),
        config.permissions.rules.clone(),
    ));
    let prompter: Arc<dyn Prompter> = Arc::new(DenyPrompter);
    let retry = providers::RetryPolicy::from_resolved(&resolved);

    let runtime = Arc::new(crate::subagents::AgentRuntime {
        provider: provider.clone(),
        permissions: permissions.clone(),
        prompter: prompter.clone(),
        base_system: system.clone(),
        cwd: cwd.clone(),
        max_tokens: resolved.max_tokens,
        temperature: resolved.temperature,
        thinking_budget: resolved.thinking_budget,
        retry,
        skills: skills.clone(),
        mcp: Some(mcp),
        agents,
    });
    registry.register(Arc::new(crate::subagents::AgentTool::new(runtime)));

    let agent_options = AgentOptions {
        provider,
        tools: Arc::new(registry),
        permissions,
        prompter,
        system,
        cwd: cwd.clone(),
        max_tokens: resolved.max_tokens,
        temperature: resolved.temperature,
        thinking_budget: resolved.thinking_budget,
        max_steps: options.max_steps,
        session: Some(session),
        messages,
        retry,
    };

    let agent = Agent::new(agent_options);
    let (tx, rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();

    let format = options.output_format.clone();
    let consumer = tokio::spawn(consume_events(rx, format));

    let prompt = options.prompt.clone();
    let task_tx = tx.clone();
    let task_cancel = cancel.clone();
    let mut agent_task = tokio::spawn(async move {
        let mut agent = agent;
        agent.run_turn(&prompt, &task_tx, &task_cancel).await
    });
    drop(tx);

    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(options.timeout_secs.max(1)),
        &mut agent_task,
    )
    .await;

    if outcome.is_err() {
        // Give running tools a moment to kill their child processes.
        cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(3), &mut agent_task).await;
    }

    let collected = consumer
        .await
        .map_err(|error| AppError::runtime(format!("event consumer failed: {error}")))?;
    let duration_ms = started.elapsed().as_millis() as u64;

    let result = match outcome {
        Err(_elapsed) => {
            return Err(AppError::Timeout(format!(
                "turn exceeded {}s",
                options.timeout_secs
            )));
        }
        Ok(Err(join_error)) => {
            return Err(AppError::runtime(format!("agent task failed: {join_error}")));
        }
        Ok(Ok(result)) => result,
    };

    match result {
        Err(error) => return Err(error),
        Ok(stop_reason) => {
            if matches!(stop_reason, crate::protocol::StopReason::Error) {
                return Err(AppError::runtime("model reported an error"));
            }
        }
    }

    if let Some(path) = &options.output_last_message {
        std::fs::write(path, &collected.final_text)?;
    }

    let reported_model = collected
        .model
        .clone()
        .unwrap_or_else(|| resolved.model.clone());

    match options.output_format.as_str() {
        "json" => {
            let payload = json!({
                "schemaVersion": 1,
                "type": "exec.result",
                "status": "success",
                "output": collected.final_text,
                "error": null,
                "model": reported_model,
                "usage": {"input": collected.input_tokens, "output": collected.output_tokens},
                "durationMs": duration_ms,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        "stream-json" => {
            let payload = json!({
                "type": "exec.completed",
                "status": "success",
                "output": collected.final_text,
                "model": reported_model,
                "usage": {"input": collected.input_tokens, "output": collected.output_tokens},
                "durationMs": duration_ms,
            });
            println!("{}", serde_json::to_string(&payload)?);
        }
        _ => {
            if !collected.final_text.is_empty() {
                println!("{}", collected.final_text);
            }
        }
    }

    Ok(())
}

struct Collected {
    final_text: String,
    input_tokens: u64,
    output_tokens: u64,
    model: Option<String>,
}

async fn consume_events(mut rx: mpsc::Receiver<AgentEvent>, format: String) -> Collected {
    let stream_json = format == "stream-json";
    let mut current = String::new();
    let mut last_turn = String::new();
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut model = None;

    while let Some(event) = rx.recv().await {
        if stream_json
            && let Some(value) = event_to_json(&event) {
                println!("{value}");
            }
        match event {
            AgentEvent::TurnStart { .. } => current.clear(),
            AgentEvent::TextDelta(text) => current.push_str(&text),
            AgentEvent::TurnEnd { .. } => {
                if !current.trim().is_empty() {
                    last_turn = current.clone();
                }
                current.clear();
            }
            AgentEvent::ModelName(name) => {
                if !name.is_empty() {
                    model = Some(name);
                }
            }
            AgentEvent::Usage { input, output } => {
                if input > 0 {
                    input_tokens = input;
                }
                if output > 0 {
                    output_tokens = output;
                }
            }
            AgentEvent::Retry {
                attempt,
                max_retries,
                delay_ms,
                error,
            } => {
                // stream-json already emitted the structured event above.
                if !stream_json {
                    eprintln!(
                        "[retry {attempt}/{max_retries} in {:.1}s] {error}",
                        delay_ms as f64 / 1000.0
                    );
                }
            }
            _ => {}
        }
    }

    Collected {
        final_text: last_turn,
        input_tokens,
        output_tokens,
        model,
    }
}

fn event_to_json(event: &AgentEvent) -> Option<serde_json::Value> {
    let value = match event {
        AgentEvent::TurnStart { step } => json!({"type": "turn_start", "step": step}),
        AgentEvent::TextDelta(text) => json!({"type": "text_delta", "text": text}),
        AgentEvent::ThinkingDelta(text) => json!({"type": "thinking_delta", "text": text}),
        AgentEvent::ToolCall { id, name, summary } => {
            json!({"type": "tool_call", "id": id, "name": name, "summary": summary})
        }
        AgentEvent::ToolStart { id, name } => {
            json!({"type": "tool_start", "id": id, "name": name})
        }
        AgentEvent::ToolEnd {
            id,
            name,
            is_error,
            ..
        } => json!({"type": "tool_end", "id": id, "name": name, "isError": is_error}),
        AgentEvent::Usage { input, output } => {
            json!({"type": "usage", "input": input, "output": output})
        }
        AgentEvent::TurnEnd { stop_reason } => {
            json!({"type": "turn_end", "stopReason": format!("{stop_reason:?}")})
        }
        AgentEvent::ModelName(name) => json!({"type": "model", "model": name}),
        AgentEvent::Retry {
            attempt,
            max_retries,
            delay_ms,
            error,
        } => json!({
            "type": "retry",
            "attempt": attempt,
            "maxRetries": max_retries,
            "delayMs": delay_ms,
            "error": error,
        }),
        AgentEvent::CompactStart { messages, tokens } => json!({
            "type": "compact_start",
            "messages": messages,
            "tokens": tokens,
        }),
        AgentEvent::CompactEnd(outcome) => match outcome {
            CompactOutcome::Compacted(report) => json!({
                "type": "compact_end",
                "status": "compacted",
                "messagesBefore": report.messages_before,
                "messagesAfter": report.messages_after,
                "tokensBefore": report.tokens_before,
                "tokensAfter": report.tokens_after,
                "summaryChars": report.summary_chars,
                "truncated": report.truncated,
            }),
            CompactOutcome::Empty => json!({"type": "compact_end", "status": "empty"}),
            CompactOutcome::Cancelled => {
                json!({"type": "compact_end", "status": "cancelled"})
            }
        },
        AgentEvent::Done { stop_reason } => {
            json!({"type": "done", "stopReason": format!("{stop_reason:?}")})
        }
        AgentEvent::Todos(todos) => json!({"type": "todos", "todos": todos}),
        AgentEvent::Team(snapshot) => json!({"type": "team", "team": snapshot}),
        AgentEvent::Error(message) => json!({"type": "error", "message": message}),
    };
    Some(value)
}

fn open_session(
    options: &ExecOptions,
    cwd: &std::path::Path,
    provider: &str,
    model: &str,
) -> Result<Session> {
    if let Some(id) = &options.session_id {
        return Session::open(id);
    }
    if options.continue_
        && let Some(id) = Session::latest_for_cwd(cwd)? {
            return Session::open(&id);
        }
    Session::create(cwd, provider, model)
}
