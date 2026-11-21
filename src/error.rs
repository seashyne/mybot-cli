use std::fmt;

/// Application-level error with a stable process exit code.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("usage: {0}")]
    Usage(String),
    #[error("config: {0}")]
    Config(String),
    #[error("runtime: {0}")]
    Runtime(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("limit: {0}")]
    Limit(String),
    #[error("cancelled")]
    Cancelled,
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::Usage(_) => 2,
            AppError::Config(_) => 3,
            AppError::Runtime(_) => 4,
            AppError::Timeout(_) => 6,
            AppError::Limit(_) => 7,
            AppError::Cancelled => 130,
        }
    }

    pub fn usage(msg: impl fmt::Display) -> Self {
        AppError::Usage(msg.to_string())
    }

    pub fn config(msg: impl fmt::Display) -> Self {
        AppError::Config(msg.to_string())
    }

    pub fn runtime(msg: impl fmt::Display) -> Self {
        AppError::Runtime(msg.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Runtime(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        AppError::Runtime(format!("json: {value}"))
    }
}
