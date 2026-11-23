use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::super::{App, Ctx, Entry};
use crate::agent::AgentEvent;
use crate::tools::bash::BashTool;
use crate::tools::checkpoint::CheckpointStore;
use crate::tools::{Tool, ToolCtx};

const MAX_FIND_MATCHES: usize = 20;

impl App {
    pub(crate) fn copy_command(&mut self, args: &str) {
        let mode = args.trim().to_ascii_lowercase();
        let (label, payload) = if mode == "code" {
            match last_code_block(&self.transcript) {
                Some(code) => ("code block", code),
                None => {
                    self.info("no code block in transcript yet");
                    return;
                }
            }
        } else if mode.is_empty() || mode == "all" || mode == "last" {
            match last_assistant_text(&self.transcript) {
                Some(text) => ("last reply", text.to_string()),
                None => {
                    self.info("no assistant reply to copy yet");
                    return;
                }
            }
        } else {
            self.info("usage: /copy [code]");
            return;
        };

        let bytes = payload.len();
        match write_clipboard(&payload) {
            Ok(tool) => self.info(format!("copied {label} ({bytes} bytes via {tool})")),
            Err(err) => self.info(err),
        }
    }

    pub(super) fn find_command(&mut self, args: &str) {
        let query = args.trim();
        if query.is_empty() {
            self.info("usage: /find <text>");
            return;
        }
        let needle = query.to_ascii_lowercase();
        let mut hits = Vec::new();

        for (idx, entry) in self.transcript.iter().enumerate() {
            let (tag, body) = match entry {
                Entry::User(text) => ("user", text.as_str()),
                Entry::Assistant(text) => ("assistant", text.as_str()),
                Entry::Tool {
                    name,
                    summary,
                    content,
                    ..
                } => {
                    let combined = format!("{name} {summary}\n{content}");
                    if combined.to_ascii_lowercase().contains(&needle) {
                        for line in combined.lines() {
                            if line.to_ascii_lowercase().contains(&needle) {
                                hits.push(format!(
                                    "#{} [tool:{}] {}",
                                    idx + 1,
                                    name,
                                    snippet(line.trim(), 96)
                                ));
                                if hits.len() >= MAX_FIND_MATCHES {
                                    break;
                                }
                            }
                        }
                    }
                    continue;
                }
                Entry::Info(text) => ("info", text.as_str()),
                Entry::Error(text) => ("error", text.as_str()),
            };

            for line in body.lines() {
                if line.to_ascii_lowercase().contains(&needle) {
                    hits.push(format!(
                        "#{} [{tag}] {}",
                        idx + 1,
                        snippet(line.trim(), 96)
                    ));
                    if hits.len() >= MAX_FIND_MATCHES {
                        break;
                    }
                }
            }
            if hits.len() >= MAX_FIND_MATCHES {
                break;
            }
        }

        if hits.is_empty() {
            self.info(format!("no transcript matches for '{query}'"));
        } else {
            let mut report = format!("matches for '{query}' ({}):\n", hits.len());
            for hit in hits {
                report.push_str(&format!("  {hit}\n"));
            }
            self.transcript.push(Entry::Info(report.trim_end().to_string()));
        }
    }

    pub(super) fn test_command(&mut self, args: &str, ctx: &Ctx) {
        let cmd_str = if !args.trim().is_empty() {
            args.trim().to_string()
        } else if let Some(detected) = detect_test_command(&self.cwd) {
            detected.to_string()
        } else {
            self.info("no test suite detected; run /test <command>");
            return;
        };

        let id = format!("slash_test_{}", self.transcript.len());
        self.transcript.push(Entry::Tool {
            id: id.clone(),
            name: "test".into(),
            summary: format!("$ {cmd_str}"),
            content: "running…".into(),
            is_error: false,
            running: true,
            expanded: true,
        });
        self.info(format!("running: {cmd_str}"));

        let tx = ctx.tx.clone();
        let tool_ctx = ToolCtx {
            cwd: self.cwd.clone(),
            cancel: self.cancel.clone(),
            todos: Arc::new(Mutex::new(Vec::new())),
            checkpoints: CheckpointStore::new(),
            sandbox: ctx.permissions.sandbox(),
        };

        tokio::spawn(async move {
            let out = BashTool
                .execute(&serde_json::json!({ "command": cmd_str }), &tool_ctx)
                .await;
            let _ = tx
                .send(AgentEvent::ToolEnd {
                    id,
                    name: "test".into(),
                    content: out.content,
                    is_error: out.is_error,
                })
                .await;
        });
    }

    pub(super) fn diff_command(&mut self) {
        let report = collect_git_diff(&self.cwd);
        self.transcript.push(Entry::Info(report));
    }
}

pub(crate) fn detect_test_command(cwd: &Path) -> Option<&'static str> {
    if cwd.join("Cargo.toml").exists() {
        Some("cargo test")
    } else if cwd.join("package.json").exists() {
        Some("npm test")
    } else if cwd.join("go.mod").exists() {
        Some("go test ./...")
    } else if cwd.join("pyproject.toml").exists() || cwd.join("pytest.ini").exists() {
        Some("pytest")
    } else if cwd.join("Makefile").exists() {
        Some("make test")
    } else {
        None
    }
}

fn collect_git_diff(cwd: &Path) -> String {
    let Ok(status) = std::process::Command::new("git")
        .args(["status", "-sb"])
        .current_dir(cwd)
        .output()
    else {
        return "git is not installed".into();
    };
    if !status.status.success() {
        return format!("not a git repository: {}", cwd.display());
    }

    let mut out = String::from("git status:\n");
    for line in String::from_utf8_lossy(&status.stdout).lines() {
        out.push_str(&format!("  {line}\n"));
    }

    if let Ok(stat) = std::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(cwd)
        .output()
    {
        let stat_text = String::from_utf8_lossy(&stat.stdout);
        if !stat_text.trim().is_empty() {
            out.push_str("\ngit diff --stat:\n");
            for line in stat_text.lines().take(30) {
                out.push_str(&format!("  {line}\n"));
            }
        }
    }
    out.trim_end().to_string()
}

pub(crate) fn last_assistant_text(transcript: &[Entry]) -> Option<&str> {
    transcript.iter().rev().find_map(|entry| match entry {
        Entry::Assistant(text) if !text.trim().is_empty() => Some(text.as_str()),
        _ => None,
    })
}

pub(crate) fn last_code_block(transcript: &[Entry]) -> Option<String> {
    for entry in transcript.iter().rev() {
        let Entry::Assistant(text) = entry else {
            continue;
        };
        if let Some(block) = extract_last_fence(text) {
            return Some(block);
        }
    }
    None
}

fn extract_last_fence(markdown: &str) -> Option<String> {
    let mut in_fence = false;
    let mut current: Vec<&str> = Vec::new();
    let mut last: Option<String> = None;

    for raw in markdown.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            if in_fence {
                last = Some(current.join("\n"));
                current.clear();
                in_fence = false;
            } else {
                in_fence = true;
            }
        } else if in_fence {
            current.push(raw);
        }
    }
    last
}

fn write_clipboard(text: &str) -> Result<&'static str, String> {
    #[cfg(target_os = "linux")]
    let candidates: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    #[cfg(target_os = "macos")]
    let candidates: &[(&str, &[&str])] = &[("pbcopy", &[])];
    #[cfg(target_os = "windows")]
    let candidates: &[(&str, &[&str])] = &[("clip", &[])];
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let candidates: &[(&str, &[&str])] = &[];

    for &(bin, args) in candidates {
        let Ok(mut child) = std::process::Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take()
            && stdin.write_all(text.as_bytes()).is_ok()
        {
            drop(stdin);
            if child.wait().is_ok_and(|s| s.success()) {
                return Ok(bin);
            }
        }
    }

    Err("no clipboard tool available (install wl-clipboard or xclip on Linux)".into())
}

fn snippet(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_last_fenced_code_block() {
        let transcript = vec![Entry::Assistant(
            "first:\n```rs\nlet a = 1;\n```\nsecond:\n```sh\necho hi\n```\n".into(),
        )];
        assert_eq!(last_code_block(&transcript).as_deref(), Some("echo hi"));
        assert!(last_assistant_text(&transcript).unwrap().contains("first:"));
    }

    #[test]
    fn find_command_adds_matching_entries_to_transcript() {
        let mut app = crate::tui::app::tests::test_app();
        app.transcript.push(Entry::User("check Cargo.toml".into()));
        app.transcript
            .push(Entry::Assistant("Updated Cargo.toml dependencies".into()));

        app.find_command("cargo.toml");
        match app.transcript.last() {
            Some(Entry::Info(text)) => {
                assert!(text.contains("matches for 'cargo.toml' (2)"));
                assert!(text.contains("[user]"));
                assert!(text.contains("[assistant]"));
            }
            _ => panic!("expected search results"),
        }
    }

    #[test]
    fn detects_test_command_for_common_project_types() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_test_command(dir.path()), None);
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"a\"\n").unwrap();
        assert_eq!(detect_test_command(dir.path()), Some("cargo test"));
    }
}
