use futures_util::StreamExt;

use crate::cli::ProviderCommand;
use crate::config::{Config, ProviderConfig, ProviderKind};
use crate::error::{AppError, Result};

pub(super) async fn provider_command(command: &ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::List => {
            let config = Config::load_raw()?;
            println!("config: {}", crate::config::config_path().display());
            println!();
            for (name, provider) in &config.providers {
                let key_env = provider
                    .api_key_env
                    .clone()
                    .unwrap_or_else(|| provider.kind.default_api_key_env().to_string());
                let key_state = if provider.api_key.as_deref().is_some_and(|k| !k.is_empty()) {
                    "key in config"
                } else if std::env::var(&key_env).map(|v| !v.is_empty()).unwrap_or(false) {
                    "env set"
                } else {
                    "no key"
                };
                let default_marker = if config.default_provider.as_deref() == Some(name) {
                    " *"
                } else {
                    ""
                };
                println!(
                    "{name}{default_marker}\n  kind:    {:?}\n  model:   {}\n  base:    {}\n  key:     {} ({key_env})",
                    provider.kind,
                    provider.model.as_deref().unwrap_or("(default)"),
                    provider
                        .base_url
                        .clone()
                        .unwrap_or_else(|| provider.kind.default_base_url().to_string()),
                    key_state
                );
            }
            Ok(())
        }
        ProviderCommand::Add {
            name,
            kind,
            base_url,
            api_key_env,
            api_key_command,
            model,
        } => {
            let mut config = Config::load_raw()?;
            let kind = match kind.as_deref() {
                None => ProviderKind::OpenAi,
                Some(value) => match value.to_ascii_lowercase().as_str() {
                    "anthropic" => ProviderKind::Anthropic,
                    "openai" | "openai-compatible" => ProviderKind::OpenAi,
                    other => {
                        return Err(AppError::usage(format!(
                            "unknown provider kind '{other}' (expected anthropic or openai)"
                        )));
                    }
                },
            };
            let provider = ProviderConfig {
                base_url: base_url.clone(),
                api_key_env: api_key_env.clone(),
                api_key_command: api_key_command.clone(),
                model: model.clone(),
                ..ProviderConfig::defaults_for(kind)
            };
            config.providers.insert(name.clone(), provider);
            config.save()?;
            println!(
                "Saved provider '{name}' to {}",
                crate::config::config_path().display()
            );
            Ok(())
        }
        ProviderCommand::Remove { name } => {
            let mut config = Config::load_raw()?;
            if config.providers.remove(name).is_none() {
                return Err(AppError::usage(format!("unknown provider '{name}'")));
            }
            config.save()?;
            println!("Removed provider '{name}'");
            Ok(())
        }
        ProviderCommand::Use { name } => {
            let mut config = Config::load_raw()?;
            if !config.providers.contains_key(name) {
                return Err(AppError::usage(format!("unknown provider '{name}'")));
            }
            config.default_provider = Some(name.clone());
            config.save()?;
            println!("Default provider is now '{name}'");
            Ok(())
        }
        ProviderCommand::Test { name } => provider_test(name.as_deref()).await,
    }
}

async fn provider_test(name: Option<&str>) -> Result<()> {
    let mut config = Config::load()?;
    if let Some(provider) = name {
        if !config.providers.contains_key(provider) {
            return Err(AppError::usage(format!("unknown provider '{provider}'")));
        }
        config.default_provider = Some(provider.to_string());
    }
    let resolved = config.resolve(None)?;
    let provider = crate::providers::build(&resolved)?;

    let request = crate::providers::CompletionRequest {
        model: resolved.model.clone(),
        system: Vec::new(),
        messages: vec![crate::protocol::Message::user("ping")],
        tools: Vec::new(),
        max_tokens: 8,
        temperature: None,
        thinking_budget: None,
    };

    let cancel = tokio_util::sync::CancellationToken::new();
    let mut stream = provider.stream(request, cancel).await?;
    let mut got_event = false;
    while let Some(item) = stream.next().await {
        item?;
        got_event = true;
    }

    if got_event {
        println!("OK: {} / {}", resolved.name, resolved.model);
        Ok(())
    } else {
        Err(AppError::runtime("provider returned an empty stream"))
    }
}
