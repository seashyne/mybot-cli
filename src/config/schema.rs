use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use super::provider::{ProviderConfig, ProviderKind};

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
    pub sandbox: super::SandboxMode,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Auto,
            sandbox: super::SandboxMode::Auto,
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
        let text = toml::to_string_pretty(self).map_err(|e| {
            crate::error::AppError::config(format!("cannot serialise config: {e}"))
        })?;
        write_restricted(&path, &text)?;
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

    /// True when at least one provider has a key or points at a local server,
    /// i.e. `resolve` can succeed without asking the user anything.
    pub fn is_configured(&self) -> bool {
        self.providers.values().any(|config| {
            let base_url = config
                .base_url
                .clone()
                .unwrap_or_else(|| config.kind.default_base_url().to_string());
            resolve_api_key(config).is_some() || is_local_url(&base_url)
        })
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
    fn unconfigured_without_keys_or_local_urls() {
        let config = Config::default();
        assert!(!config.is_configured());
    }

    #[test]
    fn local_provider_counts_as_configured() {
        let mut config = Config::default();
        config
            .providers
            .get_mut("openai")
            .unwrap()
            .base_url = Some("http://127.0.0.1:1234/v1".into());
        assert!(config.is_configured());
    }

    #[test]
    fn permission_mode_parses_aliases() {
        assert_eq!(PermissionMode::parse("smart"), Some(PermissionMode::Auto));
        assert_eq!(PermissionMode::parse("bypass"), Some(PermissionMode::Full));
        assert_eq!(PermissionMode::parse("nope"), None);
    }

    #[cfg(unix)]
    #[test]
    fn restricted_write_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/config.toml");
        write_restricted(&file, "api_key = \"secret\"").unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(file.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
    }
}
