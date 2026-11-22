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
        "minimax" => &[
            "MiniMax-M2.5",
            "MiniMax-M2.1",
            "MiniMax-M2",
            "M3.1-Flash",
            "MiniMax-Text-01",
        ],
        _ => &[],
    }
}

/// Approximate context window size in tokens for well-known model families.
pub fn context_window(model: &str) -> Option<u64> {
    let lower = model.trim().to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if base.is_empty() || base == "auto" {
        return None;
    }

    if base.contains("m3.1")
        || base.contains("minimax-text-01")
        || base.starts_with("gemini-2.")
        || base.starts_with("gemini-1.5")
        || base.starts_with("gpt-4.1")
    {
        return Some(1_000_000);
    }
    if base.starts_with("gpt-5") {
        return Some(400_000);
    }
    if base.starts_with("grok-4") {
        return Some(256_000);
    }
    if base.starts_with("claude-")
        || base.starts_with("o1")
        || base.starts_with("o3")
        || base.starts_with("o4")
        || base.contains("minimax-m2")
    {
        return Some(200_000);
    }
    if base.contains("32k") {
        return Some(32_000);
    }
    if base.contains("8k") {
        return Some(8_000);
    }
    if base.starts_with("gpt-4o")
        || base.starts_with("gpt-4-turbo")
        || base.starts_with("deepseek")
        || base.starts_with("kimi")
        || base.starts_with("moonshot-v1-128k")
        || base.starts_with("qwen")
        || base.starts_with("glm-4")
        || base.starts_with("mistral")
        || base.starts_with("codestral")
        || base.starts_with("grok-3")
        || base.starts_with("grok-2")
        || base.starts_with("llama-3.3")
        || base.starts_with("llama-3.1")
    {
        return Some(128_000);
    }

    None
}

/// Format a token count compactly for the status bar (`1M`, `200k`, `1.4k`, `850`).
pub fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        if tokens.is_multiple_of(1_000_000) {
            format!("{}M", tokens / 1_000_000)
        } else {
            format!("{:.1}M", tokens as f64 / 1_000_000.0)
        }
    } else if tokens >= 1_000 {
        if tokens >= 10_000 || tokens.is_multiple_of(1_000) {
            format!("{}k", tokens / 1_000)
        } else {
            format!("{:.1}k", tokens as f64 / 1_000.0)
        }
    } else {
        tokens.to_string()
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

    #[test]
    fn context_window_and_formatting_cover_common_models() {
        assert_eq!(context_window("M3.1-Flash"), Some(1_000_000));
        assert_eq!(context_window("claude-sonnet-4-5"), Some(200_000));
        assert_eq!(context_window("auto"), None);
        assert_eq!(format_tokens(1_000_000), "1M");
        assert_eq!(format_tokens(200_000), "200k");
        assert_eq!(format_tokens(1_500), "1.5k");
        assert_eq!(format_tokens(420), "420");
    }
}
