use crate::cli::{Cli, McpCommand};
use crate::error::{AppError, Result};
use crate::mcp::{McpConfig, McpManager, McpServer};

use super::working_dir;

pub(super) async fn mcp_command(cli: &Cli, command: &McpCommand) -> Result<()> {
    let cwd = working_dir(cli)?;
    let (config, warnings) = McpConfig::load(&cwd);
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }

    match command {
        McpCommand::List => {
            let paths: Vec<String> = McpConfig::paths(&cwd)
                .iter()
                .map(|path| path.display().to_string())
                .collect();
            println!("config: {}", paths.join(", "));

            if config.servers.is_empty() {
                println!("no MCP servers configured");
                println!();
                println!("add one to ~/.mybot/mcp.toml:");
                println!("  [servers.filesystem]");
                println!("  command = \"npx\"");
                println!("  args = [\"-y\", \"@modelcontextprotocol/server-filesystem\", \".\"]");
                return Ok(());
            }

            for (name, server) in &config.servers {
                println!(
                    "{:<16} {:<9} {}",
                    name,
                    if server.enabled { "enabled" } else { "disabled" },
                    server.transport_label()
                );
            }

            let manager = McpManager::start(&cwd).await;
            for warning in manager.warnings() {
                eprintln!("warning: {warning}");
            }
            println!();
            println!("{}", manager.describe());
            Ok(())
        }
        McpCommand::Test { name } => {
            let server = config.servers.get(name).ok_or_else(|| {
                AppError::usage(format!(
                    "unknown MCP server '{name}' (available: {})",
                    config
                        .servers
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            if !server.enabled {
                return Err(AppError::usage(format!(
                    "MCP server '{name}' is disabled"
                )));
            }

            let connected = McpServer::connect(name, server)
                .await
                .map_err(AppError::runtime)?;
            println!("OK: {name} — {} tool(s)", connected.tools.len());
            for tool in &connected.tools {
                println!("  {:<24} {}", tool.name, tool.description);
            }
            Ok(())
        }
    }
}
