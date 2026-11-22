use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "mybot",
    version,
    about = "mybot cli - a terminal AI coding agent",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Initial prompt; opens the interactive TUI
    pub prompt: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,

    /// Model to use: "model" or "provider/model"
    #[arg(short = 'm', long, global = true)]
    pub model: Option<String>,

    /// Resume a session by id
    #[arg(long, global = true)]
    pub session: Option<String>,

    /// Continue the most recent session in this directory
    #[arg(short = 'c', long = "continue", global = true)]
    pub continue_: bool,

    /// Permission mode: ask | auto | full
    #[arg(long, global = true)]
    pub permission: Option<String>,

    /// Replace the built-in system prompt
    #[arg(long = "system-prompt", global = true)]
    pub system_prompt: Option<String>,

    /// Read the system prompt from a file
    #[arg(long = "system-prompt-file", global = true)]
    pub system_prompt_file: Option<PathBuf>,

    /// Append to the built-in system prompt
    #[arg(long = "append-system-prompt", global = true)]
    pub append_system_prompt: Option<String>,

    /// Append a system prompt file to the built-in prompt
    #[arg(long = "append-system-prompt-file", global = true)]
    pub append_system_prompt_file: Option<PathBuf>,

    /// Working directory (defaults to the current directory)
    #[arg(long, global = true)]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a single prompt without the interactive UI
    Exec(ExecArgs),
    /// Create an AGENTS.md in the target directory
    Init {
        /// Directory to initialise (defaults to the current directory)
        directory: Option<PathBuf>,
    },
    /// Manage model providers
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// List and inspect skills
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    /// List, add and remove plugins
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// Inspect persistent project memory
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Manage Model Context Protocol servers
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// List configured servers and the tools they expose
    List,
    /// Connect to one server and print its tools
    Test { name: String },
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    /// List memory entries for the working directory
    List,
    /// Print one memory entry
    Show { name: String },
    /// Delete one memory entry
    Delete { name: String },
    /// Print the memory directory
    Path,
}

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// List skills found for the current directory
    List,
    /// Print a skill's instructions
    Show {
        name: String,
    },
    /// Create a starter skill
    New {
        name: String,
        /// Where to put it: project (.mybot/skills) or user (~/.mybot/skills)
        #[arg(long, default_value = "project", value_parser = ["project", "user"])]
        scope: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List installed plugins and their commands
    List,
    /// Install a plugin by copying a directory into ~/.mybot/plugins
    Add {
        path: std::path::PathBuf,
        /// Install into .mybot/plugins instead of ~/.mybot/plugins
        #[arg(long)]
        project: bool,
    },
    /// Remove an installed plugin
    Remove {
        name: String,
        #[arg(long)]
        project: bool,
    },
    /// Print a plugin command after substituting arguments
    Command {
        name: String,
        /// Arguments substituted for $ARGUMENTS
        #[arg(default_value = "")]
        arguments: String,
    },
}

#[derive(Debug, Args)]
pub struct ExecArgs {
    /// Prompt text; use --input - to read from stdin
    pub prompt: Option<String>,

    /// Prompt source: "-" reads stdin
    #[arg(long)]
    pub input: Option<String>,

    /// Output format
    #[arg(long, default_value = "text", value_parser = ["text", "json", "stream-json"])]
    pub output_format: String,

    /// Write the final assistant message to this file
    #[arg(short = 'o', long = "output-last-message")]
    pub output_last_message: Option<PathBuf>,

    /// Overall timeout in seconds
    #[arg(long, default_value_t = 600)]
    pub timeout: u64,

    /// Maximum number of model turns before giving up
    #[arg(long)]
    pub max_steps: Option<usize>,
}

#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// List configured providers
    List,
    /// Add or replace a provider
    Add {
        name: String,
        /// anthropic | openai
        #[arg(long)]
        kind: Option<String>,
        #[arg(long = "base-url")]
        base_url: Option<String>,
        #[arg(long = "api-key-env")]
        api_key_env: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// Remove a provider
    Remove { name: String },
    /// Check that a provider is reachable and configured
    Test { name: Option<String> },
    /// Set the default provider
    Use { name: String },
}

impl Cli {
    /// Permission mode from the CLI flag, if valid.
    pub fn permission_mode(&self) -> crate::error::Result<Option<crate::config::PermissionMode>> {
        match self.permission.as_deref() {
            None => Ok(None),
            Some(value) => crate::config::PermissionMode::parse(value)
                .map(Some)
                .ok_or_else(|| {
                    crate::error::AppError::usage(format!(
                        "invalid --permission '{value}' (expected ask, auto, or full)"
                    ))
                }),
        }
    }
}
