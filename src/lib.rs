pub mod agent;
pub mod cli;
pub mod config;
pub mod error;
pub mod headless;
pub mod permissions;
pub mod prompt;
pub mod protocol;
pub mod providers;
pub mod session;
pub mod tools;

#[cfg(feature = "tui")]
pub mod tui;

pub use error::{AppError, Result};
