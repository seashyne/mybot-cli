use std::io::Read;
use std::path::Path;

use clap::Parser;
use futures_util::StreamExt;

use mybot::cli::{Cli, Command, ExecArgs, ProviderCommand};
use mybot::config::{Config, ProviderConfig, ProviderKind};
use mybot::error::{AppError, Result};
use mybot::headless::{self, ExecOptions};

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mybot: {error}");
            std::process::ExitCode::from(error.exit_code() as u8)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::runtime(format!("cannot start async runtime: {error}")))?;
    runtime.block_on(dispatch(cli))
}

async fn dispatch(cli: Cli) -> Result<()> {
    match &cli.command {
        Some(Command::Exec(args)) => exec_command(&cli, args).await,
        Some(Command::Init { directory }) => init_command(directory.as_deref()),
        Some(Command::Provider { command }) => provider_command(command).await,
        None => tui_command(cli).await,
    }
}

fn working_dir(cli: &Cli) -> Result<std::path::PathBuf> {
    let cwd = match &cli.cwd {
        Some(path) => path.clone(),
        None => std::env::current_dir()?,
    };
    if !cwd.is_dir() {
        return Err(AppError::usage(format!(
            "working directory does not exist: {}",
            cwd.display()
        )));
    }
    Ok(cwd.canonicalize().unwrap_or(cwd))
}

fn read_prompt_file(path: Option<&std::path::PathBuf>) -> Result<Option<String>> {
    match path {
        None => Ok(None),
        Some(path) => Ok(Some(std::fs::read_to_string(path).map_err(|error| {
            AppError::usage(format!("cannot read {}: {error}", path.display()))
        })?)),
    }
}

fn system_parts(cli: &Cli) -> Result<(Option<String>, Vec<String>)> {
    let mut override_prompt = cli.system_prompt.clone();
    if override_prompt.is_none() {
        override_prompt = read_prompt_file(cli.system_prompt_file.as_ref())?;
    }

    let mut append = Vec::new();
    if let Some(text) = &cli.append_system_prompt {
        append.push(text.clone());
    }
    if let Some(text) = read_prompt_file(cli.append_system_prompt_file.as_ref())? {
        append.push(text);
    }

    Ok((override_prompt, append))
}

fn resolve_prompt(args: &ExecArgs) -> Result<String> {
    if let Some(prompt) = &args.prompt {
        return Ok(prompt.clone());
    }
    match args.input.as_deref() {
        Some("-") => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            if buffer.trim().is_empty() {
                return Err(AppError::usage("no prompt received on stdin"));
            }
            Ok(buffer)
        }
        Some(text) => Ok(text.to_string()),
        None => Err(AppError::usage(
            "missing prompt: pass a prompt, --input <text>, or --input -",
        )),
    }
}

async fn exec_command(cli: &Cli, args: &ExecArgs) -> Result<()> {
    let cwd = working_dir(cli)?;
    let config = Config::load()?;
    let permission = cli
        .permission_mode()?
        .unwrap_or(config.permissions.mode);
    let (system_override, append_system) = system_parts(cli)?;

    headless::run(ExecOptions {
        prompt: resolve_prompt(args)?,
        cwd,
        model: cli.model.clone(),
        output_format: args.output_format.clone(),
        output_last_message: args.output_last_message.clone(),
        timeout_secs: args.timeout,
        max_steps: args.max_steps,
        permission,
        system_override,
        append_system,
        session_id: cli.session.clone(),
        continue_: cli.continue_,
    })
    .await
}

fn init_command(directory: Option<&Path>) -> Result<()> {
    let dir = match directory {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()?,
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("AGENTS.md");
    if path.exists() {
        println!("{} already exists", path.display());
        return Ok(());
    }
    let template = r#"# Project instructions

Describe how agents should work in this repository.

## Build and test

- Build:
- Test:

## Conventions

- 
"#;
    std::fs::write(&path, template)?;
    println!("Created {}", path.display());
    Ok(())
}

async fn provider_command(command: &ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::List => {
            let config = Config::load_raw()?;
            println!("config: {}", mybot::config::config_path().display());
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
                model: model.clone(),
                ..ProviderConfig::defaults_for(kind)
            };
            config.providers.insert(name.clone(), provider);
            config.save()?;
            println!(
                "Saved provider '{name}' to {}",
                mybot::config::config_path().display()
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
    let config = Config::load()?;
    let resolved = config.resolve(name)?;
    let provider = mybot::providers::build(&resolved)?;

    let request = mybot::providers::CompletionRequest {
        model: resolved.model.clone(),
        system: Vec::new(),
        messages: vec![mybot::protocol::Message::user("ping")],
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

#[cfg(feature = "tui")]
async fn tui_command(cli: Cli) -> Result<()> {
    let cwd = working_dir(&cli)?;
    let (system_override, append_system) = system_parts(&cli)?;
    mybot::tui::run(mybot::tui::TuiOptions {
        cwd,
        model: cli.model.clone(),
        permission: cli.permission_mode()?,
        system_override,
        append_system,
        session_id: cli.session.clone(),
        continue_: cli.continue_,
        initial_prompt: cli.prompt.clone(),
    })
    .await
}

#[cfg(not(feature = "tui"))]
async fn tui_command(_cli: Cli) -> Result<()> {
    Err(AppError::usage(
        "this build has no interactive UI; use `mybot exec \"<prompt>\"`",
    ))
}
