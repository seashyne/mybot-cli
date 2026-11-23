use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(
        rename = "openai",
        alias = "openai-compatible",
        alias = "openai_completions"
    )]
    OpenAi,
}

impl ProviderKind {
    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "https://api.anthropic.com",
            ProviderKind::OpenAi => "https://api.openai.com/v1",
        }
    }

    pub fn default_api_key_env(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
            ProviderKind::OpenAi => "OPENAI_API_KEY",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "claude-sonnet-4-5",
            ProviderKind::OpenAi => "gpt-4o-mini",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Optional shell command whose stdout provides the API key (e.g. `secret-tool lookup ...` or `security find-generic-password ...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_command: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub thinking_budget: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Retries after a transient failure (429, 5xx, connection error).
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_base_delay_ms")]
    pub retry_base_delay_ms: u64,
    #[serde(default = "default_retry_max_delay_ms")]
    pub retry_max_delay_ms: u64,
}

fn default_max_tokens() -> u32 {
    8192
}

fn default_max_retries() -> u32 {
    2
}

fn default_retry_base_delay_ms() -> u64 {
    500
}

fn default_retry_max_delay_ms() -> u64 {
    8000
}

impl ProviderConfig {
    pub fn defaults_for(kind: ProviderKind) -> Self {
        Self {
            kind,
            base_url: None,
            api_key_env: Some(kind.default_api_key_env().into()),
            api_key: None,
            api_key_command: None,
            model: Some(kind.default_model().into()),
            max_tokens: default_max_tokens(),
            thinking_budget: None,
            temperature: None,
            headers: BTreeMap::new(),
            max_retries: default_max_retries(),
            retry_base_delay_ms: default_retry_base_delay_ms(),
            retry_max_delay_ms: default_retry_max_delay_ms(),
        }
    }

    pub fn anthropic_default() -> Self {
        Self::defaults_for(ProviderKind::Anthropic)
    }

    pub fn openai_default() -> Self {
        Self::defaults_for(ProviderKind::OpenAi)
    }
}

/// Reasoning / thinking effort level mapped to provider thinking budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortLevel {
    Off,
    Low,
    Medium,
    High,
}

impl EffortLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "0" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" | "med" => Some(Self::Medium),
            "high" | "max" => Some(Self::High),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn budget(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Low => Some(4_096),
            Self::Medium => Some(16_384),
            Self::High => Some(32_768),
        }
    }

    pub fn from_budget(budget: Option<u32>) -> Self {
        match budget {
            None | Some(0) => Self::Off,
            Some(tokens) if tokens <= 6_000 => Self::Low,
            Some(tokens) if tokens <= 20_000 => Self::Medium,
            Some(_) => Self::High,
        }
    }

    pub fn openai_reasoning_effort(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
        }
    }
}

/// Write a sensitive config file with owner-only permissions (`0600` on Unix).
pub fn write_restricted(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        let created = !parent.exists();
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        if created {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, text)
    }
}

pub(super) fn has_key_hint(config: &ProviderConfig) -> bool {
    config.api_key.as_ref().is_some_and(|k| !k.trim().is_empty())
        || config
            .api_key_command
            .as_ref()
            .is_some_and(|c| !c.trim().is_empty())
        || resolve_env_key(config).is_some()
}

pub(super) fn resolve_api_key(config: &ProviderConfig) -> Option<String> {
    if let Some(key) = config.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        return Some(key.trim().to_string());
    }
    if let Some(key) = resolve_env_key(config) {
        return Some(key);
    }
    let command = config
        .api_key_command
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())?;
    run_key_command(command)
}

fn resolve_env_key(config: &ProviderConfig) -> Option<String> {
    let env_name = config
        .api_key_env
        .clone()
        .unwrap_or_else(|| config.kind.default_api_key_env().to_string());
    std::env::var(env_name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
}

fn run_key_command(command: &str) -> Option<String> {
    #[cfg(windows)]
    let output = std::process::Command::new("cmd")
        .args(["/C", command])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    #[cfg(not(windows))]
    let output = std::process::Command::new("sh")
        .args(["-c", command])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}
