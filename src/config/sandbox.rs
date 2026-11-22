use serde::{Deserialize, Serialize};

/// Filesystem sandbox policy for shell commands executed by `bash`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SandboxMode {
    /// Disable OS-level sandboxing and run `bash` directly.
    Off,
    /// Use `bwrap` (Linux) or `sandbox-exec` (macOS) when available, else direct.
    #[default]
    Auto,
    /// Require an OS sandbox; fail if no supported backend works.
    Strict,
}

impl SandboxMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Some(Self::Off),
            "auto" | "on" | "enabled" => Some(Self::Auto),
            "strict" | "required" => Some(Self::Strict),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Strict => "strict",
        }
    }
}
