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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Ask,
    #[default]
    Auto,
    Full,
}

impl PermissionMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ask" => Some(Self::Ask),
            "auto" | "smart" => Some(Self::Auto),
            "full" | "bypass" | "off" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Auto => "auto",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleMatcher {
    #[default]
    Tool,
    Command,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleBehavior {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub tool: String,
    #[serde(default)]
    pub matcher: RuleMatcher,
    #[serde(default)]
    pub pattern: String,
    pub behavior: RuleBehavior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    #[serde(default)]
    pub mode: PermissionMode,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Auto,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub permissions: PermissionsConfig,
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        providers.insert("anthropic".into(), ProviderConfig::anthropic_default());
        providers.insert("openai".into(), ProviderConfig::openai_default());
        Self {
            default_provider: None,
            providers,
            permissions: PermissionsConfig::default(),
        }
    }
}

/// Fully resolved provider settings ready to build a client.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub thinking_budget: Option<u32>,
    pub temperature: Option<f32>,
    pub headers: BTreeMap<String, String>,
    pub max_retries: u32,
    pub retry_base_delay_ms: u64,
    pub retry_max_delay_ms: u64,
}

pub fn is_local_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("://localhost")
        || lower.contains("://127.0.0.1")
        || lower.contains("://0.0.0.0")
        || lower.contains("://[::1]")
}

impl Config {
    /// Read `config.toml` without applying environment overrides.
    pub fn load_raw() -> crate::error::Result<Self> {
        let path = super::paths::config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path).map_err(|e| {
            crate::error::AppError::config(format!("cannot read {}: {e}", path.display()))
        })?;
        toml::from_str(&text).map_err(|e| {
            crate::error::AppError::config(format!("invalid {}: {e}", path.display()))
        })
    }

    pub fn save(&self) -> crate::error::Result<()> {
        let path = super::paths::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| {
            crate::error::AppError::config(format!("cannot serialise config: {e}"))
        })?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// Load `config.toml`, falling back to built-in defaults when absent.
    pub fn load() -> crate::error::Result<Self> {
        let mut config = Self::load_raw()?;

        if let Ok(value) = std::env::var("MYBOT_PROVIDER")
            && !value.trim().is_empty() {
                config.default_provider = Some(value.trim().to_string());
            }

        if config.providers.is_empty() {
            let defaults = Config::default();
            config.providers = defaults.providers;
        }
        Ok(config)
    }

    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    /// Resolve a provider and model from CLI arguments plus config/env.
    ///
    /// `model_arg` accepts either `model` or `provider/model`.
    pub fn resolve(&self, model_arg: Option<&str>) -> crate::error::Result<Resolved> {
        let (provider_hint, model_override) = match model_arg {
            Some(value) if value.contains('/') => {
                let (provider, model) = value.split_once('/').unwrap();
                (Some(provider.to_string()), Some(model.to_string()))
            }
            Some(value) => (None, Some(value.to_string())),
            None => (None, None),
        };

        let name = provider_hint
            .or_else(|| self.default_provider.clone())
            .or_else(|| std::env::var("MYBOT_PROVIDER").ok().filter(|v| !v.is_empty()))
            .or_else(|| {
                self.providers
                    .iter()
                    .find(|(_, cfg)| resolve_api_key(cfg).is_some())
                    .map(|(name, _)| name.clone())
            })
            .ok_or_else(|| {
                crate::error::AppError::config(
                    "no provider configured; set ANTHROPIC_API_KEY or OPENAI_API_KEY, or add a provider to ~/.mybot/config.toml",
                )
            })?;

        let config = self.providers.get(&name).ok_or_else(|| {
            crate::error::AppError::config(format!(
                "unknown provider '{name}' (available: {})",
                self.provider_names().join(", ")
            ))
        })?;

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| config.kind.default_base_url().to_string());

        let api_key = match resolve_api_key(config) {
            Some(key) => key,
            None if is_local_url(&base_url) => String::new(),
            None => {
                return Err(crate::error::AppError::config(format!(
                    "missing API key for provider '{name}': set {}",
                    config
                        .api_key_env
                        .clone()
                        .unwrap_or_else(|| config.kind.default_api_key_env().to_string())
                )));
            }
        };

        let model = model_override
            .or_else(|| std::env::var("MYBOT_MODEL").ok().filter(|v| !v.is_empty()))
            .or_else(|| config.model.clone())
            .unwrap_or_else(|| config.kind.default_model().to_string());

        Ok(Resolved {
            name,
            kind: config.kind,
            base_url,
            api_key,
            model,
            max_tokens: config.max_tokens,
            thinking_budget: config.thinking_budget,
            temperature: config.temperature,
            headers: config.headers.clone(),
            max_retries: config.max_retries,
            retry_base_delay_ms: config.retry_base_delay_ms,
            retry_max_delay_ms: config.retry_max_delay_ms,
        })
    }
}

fn resolve_api_key(config: &ProviderConfig) -> Option<String> {
    if let Some(key) = config.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        return Some(key.trim().to_string());
    }
    let env_name = config
        .api_key_env
        .clone()
        .unwrap_or_else(|| config.kind.default_api_key_env().to_string());
    std::env::var(env_name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_two_providers() {
        let config = Config::default();
        assert!(config.providers.contains_key("anthropic"));
        assert!(config.providers.contains_key("openai"));
    }

    #[test]
    fn local_urls_detected() {
        assert!(is_local_url("http://localhost:11434/v1"));
        assert!(is_local_url("http://127.0.0.1:8000/v1"));
        assert!(!is_local_url("https://api.openai.com/v1"));
    }

    #[test]
    fn permission_mode_parses_aliases() {
        assert_eq!(PermissionMode::parse("smart"), Some(PermissionMode::Auto));
        assert_eq!(PermissionMode::parse("bypass"), Some(PermissionMode::Full));
        assert_eq!(PermissionMode::parse("nope"), None);
    }
}
