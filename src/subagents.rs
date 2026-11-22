//! Sub-agent definitions and the `agent` tool.
//!
//! Definitions are markdown files with frontmatter:
//!
//! ```text
//! ---
//! name: explore
//! description: Read-only search across the repository
//! tools: read, grep, glob
//! ---
//! You explore the codebase and report what you find.
//! ```
//!
//! Search order: `~/.mybot/agents/*.md`, then `<cwd>/.mybot/agents/*.md`
//! (project wins on a name clash).

mod tool;
#[cfg(test)]
mod tests;

pub use tool::{AgentRuntime, AgentTool};

use std::path::{Path, PathBuf};

use crate::skills::split_frontmatter;

#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    /// `None` means every built-in tool (except `agent` itself).
    pub tools: Option<Vec<String>>,
    pub body: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct AgentSet {
    pub agents: Vec<AgentDefinition>,
}

impl AgentSet {
    pub fn discover(cwd: &Path) -> Self {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".mybot/agents"));
        }
        roots.push(cwd.join(".mybot/agents"));
        Self::from_roots(&roots)
    }

    pub fn from_roots(roots: &[PathBuf]) -> Self {
        let mut agents: Vec<AgentDefinition> = Vec::new();
        for root in roots {
            for agent in load_dir(root) {
                agents.retain(|existing| existing.name != agent.name);
                agents.push(agent);
            }
        }
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        Self { agents }
    }

    pub fn get(&self, name: &str) -> Option<&AgentDefinition> {
        self.agents.iter().find(|agent| agent.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    pub fn names(&self) -> Vec<&str> {
        self.agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect()
    }

    /// System-prompt section listing the available sub-agent types.
    pub fn prompt_block(&self) -> Option<String> {
        if self.agents.is_empty() {
            return None;
        }
        let mut block = String::from(
            "<subagents>\nDelegate a focused, self-contained task with the `agent` tool.\n",
        );
        for agent in &self.agents {
            block.push_str(&format!("- {}: {}\n", agent.name, agent.description));
        }
        block.push_str("</subagents>");
        Some(block)
    }
}

fn load_dir(dir: &Path) -> Vec<AgentDefinition> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
        .filter_map(|path| load_agent(&path))
        .collect()
}

fn load_agent(path: &Path) -> Option<AgentDefinition> {
    let text = std::fs::read_to_string(path).ok()?;
    let (meta, body) = split_frontmatter(&text);
    let fallback = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = meta
        .get("name")
        .map(|value| slug(value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| slug(&fallback));
    if name.is_empty() {
        return None;
    }
    let description = meta
        .get("description")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            body.lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim_start_matches('#')
                .trim()
                .to_string()
        });
    let tools = meta.get("tools").map(|value| {
        value
            .split(',')
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
    });
    Some(AgentDefinition {
        name,
        description,
        tools,
        body: body.trim().to_string(),
        path: path.to_path_buf(),
    })
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in value.trim().chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}
