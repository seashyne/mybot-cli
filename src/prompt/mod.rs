pub mod context;

use std::path::PathBuf;

pub use context::{global_context, project_context};

pub struct PromptOptions {
    pub cwd: PathBuf,
    /// Replaces the built-in prompt entirely when set.
    pub system_override: Option<String>,
    pub append: Vec<String>,
    /// `<skills>` section listing what the `skill` tool can load.
    pub skills: Option<String>,
    /// `<memory>` section indexing durable project notes.
    pub memory: Option<String>,
    /// `<subagents>` section listing agent definitions.
    pub agents: Option<String>,
}

const BASE_PROMPT: &str = r#"You are mybot, a terminal coding agent working inside the user's project.

## How you work
- Inspect before you change: read files and search the codebase before editing.
- Make small, focused changes and match the existing style of the code.
- Never invent file contents. Read the file first.
- After changing code, verify it: run the relevant build, test, or lint command when one exists.
- When a task has several steps, keep a todo list with `todowrite` and keep exactly one item in progress.
- Ask before destructive or irreversible actions.

## Tools
- `read` reads a file with line numbers; use offset/limit for large files.
- `write` creates or overwrites a whole file.
- `edit` replaces exact text; include enough context for a unique match.
- `bash` runs shell commands in the project directory.
- `bg_start`, `bg_output`, `bg_stop` run and inspect long-running background processes (dev servers, watchers).
- `grep` searches file contents, `glob` finds files by pattern.
- `web_fetch` fetches public http/https documentation or pages (asks permission).
- `memory` reads and updates durable project notes.
- `agent` delegates a focused task to a sub-agent when `<subagents>` is listed.
- Team tools `task_create`, `task_list`, `task_update`, `send_message`, `check_messages`, and `spawn_teammate` coordinate background teammates.
- Tools named `mcp__<server>__<tool>` come from configured MCP servers; read their descriptions before calling.

## Memory
- Durable notes about this project are listed in the `<memory>` section when present. Read an entry with the `memory` tool before relying on it.
- Save decisions, constraints, and user preferences that will still matter in a later session.
- Never store credentials, keys, tokens, or private personal data in memory.

## Team
- When work splits into independent parts, put them on the shared board with `task_create`, then `spawn_teammate` one background teammate per part.
- Give every teammate a complete assignment: goal, files or areas to inspect, and what to report.
- Collect reports with `check_messages`; `wait_secs` waits for teammates to finish first.
- Keep ownership clear and do not duplicate a teammate's work. You still own the final answer and verification.

## Output
- Always reply in the exact language the user writes in (for example, reply in Thai when the user writes in Thai, and in English when the user writes in English). Never switch to Chinese or any other language unless the user explicitly writes in or asks for that language.
- Be concise. Explain only what the user needs to know.
- Use markdown. Reference code as `path:line`.
- When you finish, state what changed and what you verified."#;

pub fn build(options: &PromptOptions) -> Vec<String> {
    if let Some(system) = &options.system_override {
        let mut parts = vec![system.clone()];
        parts.extend(options.append.iter().cloned());
        return parts;
    }

    let mut parts = vec![BASE_PROMPT.to_string(), environment_block(&options.cwd)];

    if let Some(project) = project_context(&options.cwd) {
        parts.push(project);
    }
    if let Some(global) = global_context() {
        parts.push(global);
    }
    if let Some(memory) = &options.memory {
        parts.push(memory.clone());
    }
    if let Some(skills) = &options.skills {
        parts.push(skills.clone());
    }
    if let Some(agents) = &options.agents {
        parts.push(agents.clone());
    }
    parts.extend(options.append.iter().cloned());
    parts
}

fn environment_block(cwd: &std::path::Path) -> String {
    let mut block = String::from("<environment>\n");
    block.push_str(&format!("working directory: {}\n", cwd.display()));
    block.push_str(&format!(
        "platform: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    block.push_str(&format!("date: {}\n", chrono::Local::now().format("%Y-%m-%d")));
    if let Some(branch) = git_branch(cwd) {
        block.push_str(&format!("git branch: {branch}\n"));
    }
    block.push_str("</environment>");
    block
}

fn git_branch(cwd: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_base_prompt_and_environment() {
        let parts = build(&PromptOptions {
            cwd: std::env::temp_dir(),
            system_override: None,
            append: vec!["extra instructions".into()],
            skills: None,
            memory: None,
            agents: None,
        });
        assert!(parts[0].contains("mybot"));
        assert!(parts[1].contains("<environment>"));
        assert_eq!(parts.last().unwrap(), "extra instructions");
    }

    #[test]
    fn memory_block_is_included_when_present() {
        let parts = build(&PromptOptions {
            cwd: std::env::temp_dir(),
            system_override: None,
            append: vec![],
            skills: None,
            memory: Some("<memory>note</memory>".into()),
            agents: None,
        });
        assert!(parts.iter().any(|part| part.contains("<memory>note")));
    }

    #[test]
    fn agent_block_is_included_when_present() {
        let parts = build(&PromptOptions {
            cwd: std::env::temp_dir(),
            system_override: None,
            append: vec![],
            skills: None,
            memory: None,
            agents: Some("<subagents>- explore</subagents>".into()),
        });
        assert!(parts.iter().any(|part| part.contains("<subagents>")));
    }

    #[test]
    fn override_replaces_base() {
        let parts = build(&PromptOptions {
            cwd: std::env::temp_dir(),
            system_override: Some("custom".into()),
            append: vec![],
            skills: None,
            memory: Some("<memory>note</memory>".into()),
            agents: None,
        });
        assert_eq!(parts, vec!["custom".to_string()]);
    }
}
