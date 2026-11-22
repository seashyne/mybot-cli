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
