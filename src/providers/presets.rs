//! Ready-made provider settings for the services people ask for most.
//!
//! Every cloud entry speaks the OpenAI Chat Completions protocol, so they all
//! use `ProviderKind::OpenAi` except Anthropic. Model names are only defaults —
//! the setup wizard lets the user type any model the account has access to.

use crate::config::{ProviderConfig, ProviderKind};

pub struct Preset {
    /// Key used in `config.toml`.
    pub name: &'static str,
    pub label: &'static str,
    /// Short note shown in the picker.
    pub hint: &'static str,
    pub kind: ProviderKind,
    pub base_url: Option<&'static str>,
    pub model: &'static str,
    pub key_env: &'static str,
    pub needs_key: bool,
    pub local: bool,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "anthropic",
        label: "Anthropic · Claude",
        hint: "api.anthropic.com",
        kind: ProviderKind::Anthropic,
        base_url: None,
        model: "claude-sonnet-4-5",
        key_env: "ANTHROPIC_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "openai",
        label: "OpenAI · GPT",
        hint: "api.openai.com",
        kind: ProviderKind::OpenAi,
        base_url: None,
        model: "gpt-4o-mini",
        key_env: "OPENAI_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "gemini",
        label: "Google Gemini",
        hint: "generativelanguage.googleapis.com",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/"),
        model: "gemini-2.5-flash",
        key_env: "GEMINI_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "deepseek",
        label: "DeepSeek",
        hint: "api.deepseek.com",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://api.deepseek.com/v1"),
        model: "deepseek-chat",
        key_env: "DEEPSEEK_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "openrouter",
        label: "OpenRouter",
        hint: "one key, many models",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://openrouter.ai/api/v1"),
        model: "openrouter/auto",
        key_env: "OPENROUTER_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "groq",
        label: "Groq",
        hint: "api.groq.com",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://api.groq.com/openai/v1"),
        model: "llama-3.3-70b-versatile",
        key_env: "GROQ_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "mistral",
        label: "Mistral",
        hint: "api.mistral.ai",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://api.mistral.ai/v1"),
        model: "mistral-large-latest",
        key_env: "MISTRAL_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "xai",
        label: "xAI · Grok",
        hint: "api.x.ai",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://api.x.ai/v1"),
        model: "grok-2-latest",
        key_env: "XAI_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "moonshot",
        label: "Moonshot · Kimi",
        hint: "api.moonshot.cn",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://api.moonshot.cn/v1"),
        model: "moonshot-v1-8k",
        key_env: "MOONSHOT_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "qwen",
        label: "Alibaba · Qwen",
        hint: "dashscope",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        model: "qwen-max",
        key_env: "DASHSCOPE_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "zai",
        label: "Z.ai · GLM",
        hint: "api.z.ai",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://api.z.ai/api/paas/v4"),
        model: "glm-4.6",
        key_env: "ZAI_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "minimax",
        label: "MiniMax",
        hint: "api.minimax.io",
        kind: ProviderKind::OpenAi,
        base_url: Some("https://api.minimax.io/v1"),
        model: "MiniMax-M2",
        key_env: "MINIMAX_API_KEY",
        needs_key: true,
        local: false,
    },
    Preset {
        name: "lmstudio",
        label: "LM Studio",
        hint: "local · 127.0.0.1:1234",
        kind: ProviderKind::OpenAi,
        base_url: Some("http://127.0.0.1:1234/v1"),
        model: "auto",
        key_env: "LMSTUDIO_API_KEY",
        needs_key: false,
        local: true,
    },
    Preset {
        name: "ollama",
        label: "Ollama",
        hint: "local · 127.0.0.1:11434",
        kind: ProviderKind::OpenAi,
        base_url: Some("http://127.0.0.1:11434/v1"),
        model: "auto",
        key_env: "OLLAMA_API_KEY",
        needs_key: false,
        local: true,
    },
    Preset {
        name: "custom",
        label: "Custom OpenAI-compatible",
        hint: "any base URL",
        kind: ProviderKind::OpenAi,
        base_url: None,
        model: "",
        key_env: "MYBOT_API_KEY",
        needs_key: false,
        local: false,
    },
];

pub fn find(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.name == name)
}

impl Preset {
    /// Build the config entry for this preset, keeping the user's key.
    pub fn to_config(&self, base_url: Option<String>, api_key: Option<String>, model: &str) -> ProviderConfig {
        let mut config = ProviderConfig::defaults_for(self.kind);
        config.base_url = base_url.or_else(|| self.base_url.map(str::to_string));
        config.api_key_env = Some(self.key_env.to_string());
        config.api_key = api_key;
        config.model = Some(model.to_string());
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_has_a_unique_name() {
        let mut names: Vec<&str> = PRESETS.iter().map(|preset| preset.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    #[test]
    fn cloud_presets_need_a_key_and_have_a_model() {
        for preset in PRESETS
            .iter()
            .filter(|preset| !preset.local && preset.name != "custom")
        {
            assert!(preset.needs_key, "{} should need a key", preset.name);
            assert!(!preset.model.is_empty(), "{} has no model", preset.name);
        }
    }

    #[test]
    fn presets_build_usable_configs() {
        let preset = find("deepseek").unwrap();
        let config = preset.to_config(None, Some("sk-test".into()), preset.model);
        assert_eq!(config.base_url.as_deref(), Some("https://api.deepseek.com/v1"));
        assert_eq!(config.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert_eq!(config.model.as_deref(), Some("deepseek-chat"));
    }
}
