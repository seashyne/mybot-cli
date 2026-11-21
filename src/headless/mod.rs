use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentEvent, AgentOptions};
use crate::config::{Config, PermissionMode};
use crate::error::{AppError, Result};
use crate::permissions::{DenyPrompter, PermissionEngine};
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

    let system = prompt::build(&PromptOptions {
        cwd: cwd.clone(),
        system_override: options.system_override.clone(),
        append: options.append_system.clone(),
    });

    let session = open_session(&options, &cwd, &resolved.name, &resolved.model)?;
    let messages = session.messages.clone();
    let session = Arc::new(Mutex::new(session));

    let agent_options = AgentOptions {
        provider,
        tools: Arc::new(ToolRegistry::with_defaults()),
        permissions: Arc::new(PermissionEngine::new(
            options.permission,
            cwd.clone(),
            config.permissions.rules.clone(),
        )),
        prompter: Arc::new(DenyPrompter),
        system,
        cwd: cwd.clone(),
        max_tokens: resolved.max_tokens,
        temperature: resolved.temperature,
        thinking_budget: resolved.thinking_budget,
        max_steps: options.max_steps,
        session: Some(session),
        messages,
    };

    let agent = Agent::new(agent_options);
    let (tx, rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();

    let stream_json = options.output_format == "stream-json";
    let consumer = tokio::spawn(consume_events(rx, stream_json));

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

async fn consume_events(mut rx: mpsc::Receiver<AgentEvent>, stream_json: bool) -> Collected {
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
        AgentEvent::Done { stop_reason } => {
            json!({"type": "done", "stopReason": format!("{stop_reason:?}")})
        }
        AgentEvent::Todos(todos) => json!({"type": "todos", "todos": todos}),
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
