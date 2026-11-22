use std::io::Read;
use std::path::PathBuf;

use crate::cli::{Cli, Command, ExecArgs};
use crate::config::Config;
use crate::error::{AppError, Result};
use crate::headless::{self, ExecOptions};

mod init;
mod mcp;
mod memory;
mod plugin;
mod provider;
mod skill;
#[cfg(feature = "tui")]
mod tui;
mod update;
mod version;

pub async fn dispatch(cli: Cli) -> Result<()> {
    match &cli.command {
        Some(Command::Exec(args)) => exec_command(&cli, args).await,
        Some(Command::Init { directory }) => init::init_command(directory.as_deref()),
        Some(Command::Provider { command }) => provider::provider_command(command).await,
        Some(Command::Skill { command }) => skill::skill_command(&cli, command),
        Some(Command::Plugin { command }) => plugin::plugin_command(&cli, command),
        Some(Command::Memory { command }) => memory::memory_command(&cli, command),
        Some(Command::Mcp { command }) => mcp::mcp_command(&cli, command).await,
        Some(Command::Version { json }) => version::version_command(*json),
        Some(Command::Update(args)) => update::update_command(args).await,
        None => tui_command(cli).await,
    }
}

pub(super) fn working_dir(cli: &Cli) -> Result<PathBuf> {
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

fn read_prompt_file(path: Option<&PathBuf>) -> Result<Option<String>> {
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

#[cfg(feature = "tui")]
async fn tui_command(cli: Cli) -> Result<()> {
    tui::run(&cli).await
}

#[cfg(not(feature = "tui"))]
async fn tui_command(_cli: Cli) -> Result<()> {
    Err(AppError::usage(
        "this build has no interactive UI; use `mybot exec \"<prompt>\"`",
    ))
}
