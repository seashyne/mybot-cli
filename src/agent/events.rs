use crate::protocol::StopReason;
use crate::team::TeamSnapshot;
use crate::tools::TodoItem;

/// Result of a successful compaction.
#[derive(Debug, Clone)]
pub struct CompactReport {
    pub messages_before: usize,
    pub messages_after: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub summary_chars: usize,
    /// The model hit its output limit while writing the summary.
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub enum CompactOutcome {
    Compacted(CompactReport),
    /// There was no history to summarize.
    Empty,
    Cancelled,
}

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
    /// Opening the model stream failed transiently; another attempt follows.
    Retry {
        attempt: u32,
        max_retries: u32,
        delay_ms: u64,
        error: String,
    },
    /// A `/compact` run started summarizing the history.
    CompactStart {
        messages: usize,
        tokens: usize,
    },
    CompactEnd(CompactOutcome),
    /// The whole user turn finished.
    Done {
        stop_reason: StopReason,
    },
    Todos(Vec<TodoItem>),
    /// The shared team board or a teammate status changed.
    Team(TeamSnapshot),
    Error(String),
}
