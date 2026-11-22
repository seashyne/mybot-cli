use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::assembler::ToolInvocation;
use crate::agent::events::AgentEvent;
use crate::protocol::ToolResult;
use crate::tools::{ExecutionMode, ToolCtx};

use super::Agent;

impl Agent {
    pub(super) async fn execute_tools(
        &self,
        invocations: &[ToolInvocation],
        tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Vec<ToolResult> {
        let mut results: Vec<Option<ToolResult>> = vec![None; invocations.len()];
        let mut runnable: Vec<usize> = Vec::new();

        for (index, invocation) in invocations.iter().enumerate() {
            let summary = self
                .tools
                .get(&invocation.name)
                .map(|tool| match &invocation.arguments {
                    Ok(args) => tool.summary(args),
                    Err(_) => invocation.name.clone(),
                })
                .unwrap_or_else(|| invocation.name.clone());

            let _ = tx
                .send(AgentEvent::ToolCall {
                    id: invocation.id.clone(),
                    name: invocation.name.clone(),
                    summary: summary.clone(),
                })
                .await;

            let Some(tool) = self.tools.get(&invocation.name) else {
                let result = error_result(
                    invocation,
                    format!("unknown tool '{}'", invocation.name),
                );
                let _ = tx
                    .send(AgentEvent::ToolEnd {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                        content: result.content.clone(),
                        is_error: true,
                    })
                    .await;
                results[index] = Some(result);
                continue;
            };

            let args = match &invocation.arguments {
                Ok(args) => args.clone(),
                Err(error) => {
                    let result = error_result(invocation, error.clone());
                    let _ = tx
                        .send(AgentEvent::ToolEnd {
                            id: invocation.id.clone(),
                            name: invocation.name.clone(),
                            content: result.content.clone(),
                            is_error: true,
                        })
                        .await;
                    results[index] = Some(result);
                    continue;
                }
            };

            let path = tool
                .path_arg(&args)
                .map(|raw| crate::tools::resolve_path(&self.cwd, &raw));
            let detail = permission_detail(&invocation.name, &args);
            let request = crate::permissions::request_for(
                &invocation.name,
                summary.clone(),
                detail,
                &args,
                &self.cwd,
            );
            let decision = self
                .permissions
                .check(&request, path.as_deref(), self.prompter.as_ref())
                .await;

            if !decision.is_allowed() {
                let result = error_result(
                    invocation,
                    format!(
                        "permission denied for {} (mode: {})",
                        invocation.name,
                        self.permissions.mode.label()
                    ),
                );
                let _ = tx
                    .send(AgentEvent::ToolEnd {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                        content: result.content.clone(),
                        is_error: true,
                    })
                    .await;
                results[index] = Some(result);
                continue;
            }

            runnable.push(index);
        }

        let ctx = ToolCtx {
            cwd: self.cwd.clone(),
            cancel: cancel.clone(),
            todos: self.todos.clone(),
        };

        let mut cursor = 0;
        while cursor < runnable.len() {
            let index = runnable[cursor];
            let invocation = &invocations[index];
            let Some(tool) = self.tools.get(&invocation.name) else {
                cursor += 1;
                continue;
            };
            let args = invocation
                .arguments
                .clone()
                .unwrap_or(Value::Object(Default::default()));

            if tool.execution_mode() == ExecutionMode::Parallel {
                let mut batch = vec![index];
                let mut next = cursor + 1;
                while next < runnable.len() {
                    let candidate = &invocations[runnable[next]];
                    match self.tools.get(&candidate.name) {
                        Some(candidate_tool)
                            if candidate_tool.execution_mode() == ExecutionMode::Parallel =>
                        {
                            batch.push(runnable[next]);
                            next += 1;
                        }
                        _ => break,
                    }
                }

                for &item in &batch {
                    let _ = tx
                        .send(AgentEvent::ToolStart {
                            id: invocations[item].id.clone(),
                            name: invocations[item].name.clone(),
                        })
                        .await;
                }

                let futures = batch.iter().map(|&item| {
                    let tool = self
                        .tools
                        .get(&invocations[item].name)
                        .expect("tool checked above");
                    let args = invocations[item]
                        .arguments
                        .clone()
                        .unwrap_or(Value::Object(Default::default()));
                    let ctx = ctx.clone();
                    async move { tool.execute(&args, &ctx).await }
                });
                let outputs = futures_util::future::join_all(futures).await;

                for (&item, output) in batch.iter().zip(outputs) {
                    let _ = tx
                        .send(AgentEvent::ToolEnd {
                            id: invocations[item].id.clone(),
                            name: invocations[item].name.clone(),
                            content: output.content.clone(),
                            is_error: output.is_error,
                        })
                        .await;
                    results[item] = Some(ToolResult {
                        tool_call_id: invocations[item].id.clone(),
                        tool_name: invocations[item].name.clone(),
                        content: output.content,
                        is_error: output.is_error,
                    });
                }

                cursor = next;
            } else {
                let _ = tx
                    .send(AgentEvent::ToolStart {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                    })
                    .await;
                let output = tool.execute(&args, &ctx).await;
                let _ = tx
                    .send(AgentEvent::ToolEnd {
                        id: invocation.id.clone(),
                        name: invocation.name.clone(),
                        content: output.content.clone(),
                        is_error: output.is_error,
                    })
                    .await;
                results[index] = Some(ToolResult {
                    tool_call_id: invocation.id.clone(),
                    tool_name: invocation.name.clone(),
                    content: output.content,
                    is_error: output.is_error,
                });
                cursor += 1;
            }
        }

        if invocations.iter().any(|invocation| invocation.name == "todowrite") {
            let todos = self.todos.lock().await.clone();
            let _ = tx.send(AgentEvent::Todos(todos)).await;
        }

        results
            .into_iter()
            .map(|result| {
                result.unwrap_or_else(|| ToolResult {
                    tool_call_id: "unknown".into(),
                    tool_name: "unknown".into(),
                    content: "tool was not executed".into(),
                    is_error: true,
                })
            })
            .collect()
    }
}

fn error_result(invocation: &ToolInvocation, message: String) -> ToolResult {
    ToolResult {
        tool_call_id: invocation.id.clone(),
        tool_name: invocation.name.clone(),
        content: message,
        is_error: true,
    }
}

fn permission_detail(tool: &str, args: &Value) -> String {
    match tool {
        "bash" => args
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "write" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("?");
            let size = args
                .get("content")
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or(0);
            format!("write {path} ({size} bytes)")
        }
        "edit" => {
            let path = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            format!("edit {path}")
        }
        "read" | "grep" | "glob" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            format!("{tool} {path}")
        }
        _ => tool.to_string(),
    }
}
