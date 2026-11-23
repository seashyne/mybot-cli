//! Single source of truth for interactive slash commands.
//!
//! The palette, `/help`, and the dispatcher all read this catalog so a command
//! cannot exist in one place but be missing from another.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Session,
    Model,
    Context,
    Settings,
    Help,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Session,
        Category::Model,
        Category::Context,
        Category::Settings,
        Category::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Session => "Session",
            Category::Model => "Model",
            Category::Context => "Context & tools",
            Category::Settings => "Settings",
            Category::Help => "Help",
        }
    }
}

pub struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub args: &'static str,
    pub description: &'static str,
    pub category: Category,
    /// Replaces or rewinds the active session, so it waits for a turn to end.
    pub idle_only: bool,
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "help",
        aliases: &[],
        args: "",
        description: "Show commands, keys, and usage",
        category: Category::Help,
        idle_only: false,
    },
    CommandSpec {
        name: "changelog",
        aliases: &[],
        args: "",
        description: "Show recent release notes",
        category: Category::Help,
        idle_only: false,
    },
    CommandSpec {
        name: "model",
        aliases: &[],
        args: "[provider/model]",
        description: "Switch model with a searchable picker",
        category: Category::Model,
        idle_only: false,
    },
    CommandSpec {
        name: "key",
        aliases: &["login", "apikey", "auth"],
        args: "[provider] <key|url>",
        description: "Save an API key or server URL and connect",
        category: Category::Model,
        idle_only: false,
    },
    CommandSpec {
        name: "effort",
        aliases: &["thinking"],
        args: "[off|low|medium|high]",
        description: "Show or set reasoning effort level",
        category: Category::Model,
        idle_only: false,
    },
    CommandSpec {
        name: "permissions",
        aliases: &["permission", "mode"],
        args: "[ask|auto|full]",
        description: "Show or change the permission mode",
        category: Category::Settings,
        idle_only: false,
    },
    CommandSpec {
        name: "sandbox",
        aliases: &[],
        args: "[off|auto|strict]",
        description: "Show or change OS bash sandbox mode",
        category: Category::Settings,
        idle_only: false,
    },
    CommandSpec {
        name: "image",
        aliases: &["attach"],
        args: "[path|paste|clear]",
        description: "Attach an image file or clipboard screenshot",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "tools",
        aliases: &[],
        args: "",
        description: "List the tools the model can call",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "context",
        aliases: &["ctx"],
        args: "",
        description: "Show context, token, and session usage",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "status",
        aliases: &[],
        args: "",
        description: "Show provider, model, permission, and tokens",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "version",
        aliases: &[],
        args: "",
        description: "Show version, commit, and target",
        category: Category::Settings,
        idle_only: false,
    },
    CommandSpec {
        name: "sessions",
        aliases: &[],
        args: "",
        description: "List recent sessions in this directory",
        category: Category::Session,
        idle_only: false,
    },
    CommandSpec {
        name: "resume",
        aliases: &[],
        args: "<id>",
        description: "Load a previous session into this window",
        category: Category::Session,
        idle_only: true,
    },
    CommandSpec {
        name: "new",
        aliases: &["clear"],
        args: "",
        description: "Start a fresh session and clear the transcript",
        category: Category::Session,
        idle_only: true,
    },
    CommandSpec {
        name: "compact",
        aliases: &[],
        args: "",
        description: "Summarize the conversation to free context",
        category: Category::Session,
        idle_only: true,
    },
    CommandSpec {
        name: "retry",
        aliases: &[],
        args: "",
        description: "Send the last prompt again",
        category: Category::Session,
        idle_only: true,
    },
    CommandSpec {
        name: "undo",
        aliases: &["rewind"],
        args: "",
        description: "Revert file edits from the last turn",
        category: Category::Session,
        idle_only: true,
    },
    CommandSpec {
        name: "stop",
        aliases: &["cancel"],
        args: "",
        description: "Cancel the running turn",
        category: Category::Session,
        idle_only: false,
    },
    CommandSpec {
        name: "export",
        aliases: &[],
        args: "[path]",
        description: "Write the transcript to a markdown file",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "copy",
        aliases: &["yank"],
        args: "[code]",
        description: "Copy the last reply or code block",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "find",
        aliases: &["search"],
        args: "<text>",
        description: "Search messages and tool output",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "test",
        aliases: &["check"],
        args: "[command]",
        description: "Run project tests in the OS sandbox",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "diff",
        aliases: &["git"],
        args: "",
        description: "Show git status and uncommitted diff",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "config",
        aliases: &[],
        args: "",
        description: "Show config paths, providers, and rules",
        category: Category::Settings,
        idle_only: false,
    },
    CommandSpec {
        name: "doctor",
        aliases: &[],
        args: "",
        description: "Run diagnostics on config and integrations",
        category: Category::Settings,
        idle_only: false,
    },
    CommandSpec {
        name: "keys",
        aliases: &["hotkeys"],
        args: "",
        description: "Show keyboard shortcuts",
        category: Category::Settings,
        idle_only: false,
    },
    CommandSpec {
        name: "skills",
        aliases: &[],
        args: "",
        description: "List installed skills and plugin commands",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "skill",
        aliases: &[],
        args: "<name>",
        description: "Load a skill and send its instructions",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "agents",
        aliases: &[],
        args: "",
        description: "List sub-agent definitions",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "memory",
        aliases: &[],
        args: "[show|delete] <name>",
        description: "Browse or edit project memory",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "remember",
        aliases: &[],
        args: "<text>",
        description: "Save a durable note about this project",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "mcp",
        aliases: &[],
        args: "",
        description: "Show Model Context Protocol servers",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "team",
        aliases: &[],
        args: "",
        description: "Show the shared task board and teammates",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "init",
        aliases: &[],
        args: "",
        description: "Create an AGENTS.md template",
        category: Category::Context,
        idle_only: false,
    },
    CommandSpec {
        name: "quit",
        aliases: &["exit"],
        args: "",
        description: "Exit mybot",
        category: Category::Session,
        idle_only: false,
    },
];

pub fn find(name: &str) -> Option<&'static CommandSpec> {
    let name = name.trim().trim_start_matches('/');
    COMMANDS
        .iter()
        .find(|command| command.name == name || command.aliases.contains(&name))
}

/// Rank commands for a palette query (the leading `/` is optional).
pub fn matches(query: &str) -> Vec<&'static CommandSpec> {
    let query = query.trim().trim_start_matches('/');
    let mut ranked: Vec<(i32, usize, &'static CommandSpec)> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(index, command)| score(query, command).map(|score| (score, index, command)))
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, _, command)| command).collect()
}

fn score(query: &str, command: &CommandSpec) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let query = query.to_ascii_lowercase();
    let name = command.name.to_ascii_lowercase();

    if name == query {
        return Some(1000);
    }
    if name.starts_with(&query) {
        return Some(900 - name.len() as i32);
    }
    for alias in command.aliases {
        let alias = alias.to_ascii_lowercase();
        if alias == query {
            return Some(850);
        }
        if alias.starts_with(&query) {
            return Some(800 - alias.len() as i32);
        }
    }
    if name.contains(&query) {
        return Some(700 - name.len() as i32);
    }
    if is_subsequence(&query, &name) {
        return Some(500 - name.len() as i32);
    }
    if command.description.to_ascii_lowercase().contains(&query) {
        return Some(300);
    }
    None
}

fn is_subsequence(query: &str, text: &str) -> bool {
    let mut chars = text.chars();
    query
        .chars()
        .all(|wanted| chars.any(|candidate| candidate == wanted))
}

mod help;
#[cfg(test)]
mod tests;

pub use help::help_text;
