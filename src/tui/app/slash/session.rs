use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::super::{App, Ctx, Entry, short_id};
use crate::protocol::Message;
use crate::session::Session;

impl App {
    pub(super) fn new_session(&mut self, ctx: &Ctx) {
        if !self.idle() {
            return;
        }
        self.transcript.clear();
        self.streaming.clear();
        self.streaming_active = false;
        self.todos.clear();
        self.team = None;
        match Session::create(&self.cwd, &self.status.provider, &self.status.model) {
            Ok(session) => {
                self.status.session = short_id(session.id());
                let session = Arc::new(Mutex::new(session));
                let agent = ctx.agent.clone();
                tokio::spawn(async move {
                    agent.lock().await.reset(Some(session)).await;
                });
                self.info("started a new session");
            }
            Err(error) => self.info(format!("cannot create session: {error}")),
        }
    }

    pub(super) fn sessions_command(&mut self) {
        match Session::list() {
            Ok(sessions) if sessions.is_empty() => self.info("no sessions yet"),
            Ok(sessions) => {
                let mut text = String::from("recent sessions:\n");
                for header in sessions.iter().take(10) {
                    text.push_str(&format!(
                        "  {}  {}  {}\n",
                        short_id(&header.id),
                        header.created_at.format("%Y-%m-%d %H:%M"),
                        header.model
                    ));
                }
                text.push_str("resume with /resume <id> (the id prefix is enough)");
                self.transcript.push(Entry::Info(text));
            }
            Err(error) => self.info(format!("cannot list sessions: {error}")),
        }
    }

    pub(super) fn resume_command(&mut self, args: &str, ctx: &Ctx) {
        if !self.idle() {
            return;
        }
        if args.is_empty() {
            self.sessions_command();
            self.info("usage: /resume <id>");
            return;
        }
        match Session::open(args) {
            Ok(session) => {
                let id = short_id(session.id());
                let messages = session.messages.clone();
                let replay = replay_entries(&messages);
                let session = Arc::new(Mutex::new(session));
                let agent = ctx.agent.clone();
                tokio::spawn(async move {
                    agent
                        .lock()
                        .await
                        .reset_with(Some(session), messages)
                        .await;
                });
                self.transcript.clear();
                self.transcript.extend(replay);
                self.transcript.push(Entry::Info(format!(
                    "── resumed session {id} ──"
                )));
                self.streaming.clear();
                self.streaming_active = false;
                self.todos.clear();
                self.team = None;
                self.status.session = id.clone();
                self.info(format!("resumed session {id}"));
            }
            Err(error) => self.info(format!("cannot resume: {error}")),
        }
    }

    pub(super) fn compact_command(&mut self, ctx: &Ctx) {
        if !self.idle() {
            return;
        }
        self.busy = true;
        self.streaming_active = false;
        self.streaming.clear();
        self.thinking_chars = 0;
        self.cancel = CancellationToken::new();
        self.auto_scroll = true;
        self.scroll_from_bottom = 0;

        let agent = ctx.agent.clone();
        let tx = ctx.tx.clone();
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut agent = agent.lock().await;
            // Failures are reported through AgentEvent::Error.
            let _ = agent.compact(&tx, &cancel).await;
        });
        self.info("compacting…");
    }

    pub(super) fn retry_command(&mut self, ctx: &Ctx) {
        if !self.idle() {
            return;
        }
        match self.last_prompt.clone() {
            Some(prompt) => {
                self.transcript.push(Entry::Info("↻ sending the last prompt again".into()));
                self.start_turn(prompt, ctx);
            }
            None => self.info("nothing to retry yet"),
        }
    }

    pub(super) fn stop_command(&mut self) {
        if self.busy {
            self.cancel.cancel();
            self.info("cancelling…");
        } else {
            self.info("no turn is running");
        }
    }
}

/// Show the tail of a resumed conversation without replaying tool payloads.
fn replay_entries(messages: &[Message]) -> Vec<Entry> {
    let start = messages.len().saturating_sub(20);
    let mut entries = Vec::new();
    for message in &messages[start..] {
        match message {
            Message::User { content } => entries.push(Entry::User(content.clone())),
            Message::Assistant { .. } => {
                let text = message.text();
                if !text.trim().is_empty() {
                    entries.push(Entry::Assistant(text));
                }
            }
            Message::ToolResults { .. } => {}
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ToolResult;

    #[test]
    fn replay_skips_tool_results_and_empty_assistant_turns() {
        let messages = vec![
            Message::user("hello"),
            Message::Assistant {
                content: vec![crate::protocol::Block::ToolCall {
                    id: "c1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({}),
                }],
            },
            Message::ToolResults {
                results: vec![ToolResult {
                    tool_call_id: "c1".into(),
                    tool_name: "read".into(),
                    content: "data".into(),
                    is_error: false,
                }],
            },
            Message::assistant_text("done"),
        ];

        let entries = replay_entries(&messages);
        assert_eq!(entries.len(), 2);
        assert!(matches!(&entries[0], Entry::User(text) if text == "hello"));
        assert!(matches!(&entries[1], Entry::Assistant(text) if text == "done"));
    }

    #[test]
    fn replay_keeps_only_the_last_twenty_messages() {
        let messages: Vec<Message> = (0..30)
            .map(|index| Message::user(format!("message {index}")))
            .collect();

        let entries = replay_entries(&messages);
        assert_eq!(entries.len(), 20);
        assert!(matches!(&entries[0], Entry::User(text) if text == "message 10"));
    }
}
