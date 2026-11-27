pub mod agent;
pub mod cli;
pub mod commands;
pub mod compact;
pub mod config;
pub mod error;
pub mod headless;
pub mod hooks;
pub mod mcp;
pub mod memory;
pub mod permissions;
pub mod plugins;
pub mod prompt;
pub mod protocol;
pub mod providers;
pub mod session;
pub mod skills;
pub mod subagents;
pub mod team;
pub mod tools;
pub mod update;
pub mod version;

#[cfg(feature = "tui")]
pub mod tui;

pub use error::{AppError, Result};
