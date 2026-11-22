use std::path::Path;

use crate::cli::{Cli, PluginCommand};
use crate::error::{AppError, Result};

use super::working_dir;

pub(super) fn plugin_command(cli: &Cli, command: &PluginCommand) -> Result<()> {
    let cwd = working_dir(cli)?;

    match command {
        PluginCommand::List => {
            let plugins = crate::plugins::PluginSet::discover(&cwd);
            if plugins.plugins.is_empty() {
                println!("no plugins installed");
                println!();
                println!("install one with `mybot plugin add <directory>`");
                return Ok(());
            }
            for plugin in &plugins.plugins {
                let scope = if plugin.project { "project" } else { "user" };
                println!(
                    "{} {}  [{}]  {}",
                    plugin.name,
                    if plugin.version.is_empty() {
                        String::new()
                    } else {
                        format!("v{}", plugin.version)
                    },
                    scope,
                    plugin.description
                );
                println!("  {}", plugin.root.display());
            }
            if !plugins.commands.is_empty() {
                println!("\ncommands:");
                for command in &plugins.commands {
                    println!(
                        "  /{:<18} {:<12} {}",
                        command.name, command.plugin, command.description
                    );
                }
            }
        }
        PluginCommand::Add { path, project } => {
            let source = path.canonicalize().map_err(|error| {
                AppError::usage(format!("cannot read {}: {error}", path.display()))
            })?;
            if !source.is_dir() {
                return Err(AppError::usage(format!(
                    "{} is not a directory",
                    source.display()
                )));
            }
            let name = source
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .ok_or_else(|| AppError::usage("cannot determine the plugin name"))?;
            let root = if *project {
                cwd.join(".mybot/plugins")
            } else {
                let home = dirs::home_dir()
                    .ok_or_else(|| AppError::runtime("cannot find the home directory"))?;
                home.join(".mybot/plugins")
            };
            let target = root.join(&name);
            if target.exists() {
                return Err(AppError::usage(format!(
                    "{} already exists (remove it first)",
                    target.display()
                )));
            }
            std::fs::create_dir_all(&root)?;
            copy_dir(&source, &target)?;
            println!("installed {} → {}", name, target.display());
        }
        PluginCommand::Remove { name, project } => {
            let root = if *project {
                cwd.join(".mybot/plugins")
            } else {
                let home = dirs::home_dir()
                    .ok_or_else(|| AppError::runtime("cannot find the home directory"))?;
                home.join(".mybot/plugins")
            };
            let target = root.join(name);
            if !target.exists() {
                return Err(AppError::usage(format!(
                    "no plugin named '{name}' in {}",
                    root.display()
                )));
            }
            std::fs::remove_dir_all(&target)?;
            println!("removed {}", target.display());
        }
        PluginCommand::Command { name, arguments } => {
            let plugins = crate::plugins::PluginSet::discover(&cwd);
            let command = plugins.command(name).ok_or_else(|| {
                AppError::usage(format!("unknown plugin command '{name}'"))
            })?;
            println!("{}", command.render(arguments));
        }
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
