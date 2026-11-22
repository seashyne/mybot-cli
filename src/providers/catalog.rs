//! Curated model names offered by the interactive model picker.
//!
//! The catalog is only a convenience: any model id the provider accepts can
//! still be typed into the picker. Lists are ordered newest/most capable first.

pub fn models_for(provider: &str) -> &'static [&'static str] {
    match provider {
        "anthropic" => &[
            "claude-opus-4-1",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
            "claude-3-7-sonnet-latest",
            "claude-3-5-haiku-latest",
        ],
        "openai" => &[
            "gpt-5",
            "gpt-5-mini",
            "gpt-4.1",
            "gpt-4o",
            "gpt-4o-mini",
            "o3",
            "o4-mini",
        ],
        "gemini" => &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash"],
        "deepseek" => &["deepseek-chat", "deepseek-reasoner"],
        "openrouter" => &[
            "openrouter/auto",
            "anthropic/claude-sonnet-4.5",
            "openai/gpt-5",
            "google/gemini-2.5-pro",
            "deepseek/deepseek-chat",
        ],
        "groq" => &[
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "qwen/qwen3-32b",
            "moonshotai/kimi-k2-instruct",
        ],
        "mistral" => &[
            "mistral-large-latest",
            "mistral-medium-latest",
            "mistral-small-latest",
            "codestral-latest",
        ],
        "xai" => &["grok-4", "grok-3", "grok-2-latest"],
        "moonshot" => &[
            "kimi-k2-0711-preview",
            "moonshot-v1-128k",
            "moonshot-v1-32k",
            "moonshot-v1-8k",
        ],
        "qwen" => &[
            "qwen-max",
            "qwen-plus",
            "qwen3-coder-plus",
            "qwen2.5-coder-32b-instruct",
        ],
        "zai" => &["glm-4.6", "glm-4.5", "glm-4.5-air"],
        "minimax" => &["MiniMax-M2", "MiniMax-Text-01", "abab6.5s-chat"],
        _ => &[],
    }
}

/// Configured model first, then catalog entries, without duplicates.
pub fn with_configured(provider: &str, configured: Option<&str>) -> Vec<String> {
    let mut models = Vec::new();
    if let Some(model) = configured
        && !model.trim().is_empty()
        && !model.eq_ignore_ascii_case("auto")
    {
        models.push(model.trim().to_string());
    }
    for model in models_for(provider) {
        if !models.iter().any(|existing| existing == model) {
            models.push((*model).to_string());
        }
    }
    // Local servers use `auto` to mean "whatever the server has loaded".
    if models.is_empty()
        && let Some(model) = configured.filter(|model| !model.trim().is_empty())
    {
        models.push(model.trim().to_string());
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_every_cloud_preset() {
        for preset in crate::providers::presets::PRESETS
            .iter()
            .filter(|preset| !preset.local && preset.name != "custom")
        {
            assert!(
                !models_for(preset.name).is_empty(),
                "{} has no catalog entries",
                preset.name
            );
        }
    }

    #[test]
    fn configured_model_comes_first_and_is_not_duplicated() {
        let models = with_configured("anthropic", Some("claude-sonnet-4-5"));
        assert_eq!(models[0], "claude-sonnet-4-5");
        assert_eq!(
            models
                .iter()
                .filter(|model| model.as_str() == "claude-sonnet-4-5")
                .count(),
            1
        );
    }

    #[test]
    fn local_auto_stays_selectable() {
        assert_eq!(with_configured("ollama", Some("auto")), vec!["auto"]);
    }
}
