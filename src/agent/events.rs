use crate::protocol::StopReason;
use crate::tools::TodoItem;

/// Events emitted by the agent loop. The TUI and headless mode consume the same
/// stream so there is exactly one implementation of the loop.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    TurnStart {
        step: usize,
    },
    TextDelta(String),
    ThinkingDelta(String),
    /// A tool call was produced by the model (before permission/execution).
    ToolCall {
        id: String,
        name: String,
        summary: String,
    },
    ToolStart {
        id: String,
        name: String,
    },
    ToolEnd {
        id: String,
        name: String,
        content: String,
        is_error: bool,
    },
    Usage {
        input: u64,
        output: u64,
    },
    /// One model step finished (there may be more tool steps after it).
    TurnEnd {
        stop_reason: StopReason,
    },
    /// The provider reported the concrete model name (useful for "auto").
    ModelName(String),
    /// The whole user turn finished.
    Done {
        stop_reason: StopReason,
    },
    Todos(Vec<TodoItem>),
    Error(String),
}
