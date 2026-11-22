//! Model Context Protocol client.
//!
//! Servers are declared in `~/.mybot/mcp.toml` and `<project>/.mybot/mcp.toml`
//! (project wins on a name clash). Each server speaks JSON-RPC over stdio or
//! streamable HTTP; its tools are exposed as `mcp__<server>__<tool>`.

pub mod client;
pub mod config;
mod http;
mod stdio;
mod tool;

pub use client::McpToolDef;
pub use config::{McpConfig, McpServerConfig};
pub use tool::McpTool;

use std::path::Path;
use std::sync::Arc;

use crate::tools::Tool;

pub struct McpServer {
    pub name: String,
    pub tools: Vec<McpToolDef>,
    client: Arc<client::Client>,
}

impl McpServer {
    pub async fn connect(name: &str, cfg: &McpServerConfig) -> Result<Self, String> {
        let client = Arc::new(client::Client::connect(name, cfg).await?);
        let tools = client.list_tools().await?;
        Ok(Self {
            name: name.to_string(),
            tools,
            client,
        })
    }

    pub fn tool_handles(&self) -> Vec<Arc<dyn Tool>> {
        self.tools
            .iter()
            .cloned()
            .map(|definition| {
                Arc::new(McpTool::new(&self.name, definition, self.client.clone())) as Arc<dyn Tool>
            })
            .collect()
    }
}

pub struct McpManager {
    pub servers: Vec<McpServer>,
    warnings: Vec<String>,
}

impl McpManager {
    pub async fn start(cwd: &Path) -> Self {
        let (config, mut warnings) = McpConfig::load(cwd);
        let entries: Vec<(String, McpServerConfig)> = config
            .enabled()
            .map(|(name, server)| (name.to_string(), server.clone()))
            .collect();

        let results = futures_util::future::join_all(
            entries
                .iter()
                .map(|(name, server)| McpServer::connect(name, server)),
        )
        .await;

        let mut servers = Vec::new();
        for ((name, _), result) in entries.iter().zip(results) {
            match result {
                Ok(server) => servers.push(server),
                Err(error) => warnings.push(format!("mcp '{name}': {error}")),
            }
        }

        Self { servers, warnings }
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub fn tool_count(&self) -> usize {
        self.servers.iter().map(|server| server.tools.len()).sum()
    }

    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.servers
            .iter()
            .flat_map(McpServer::tool_handles)
            .collect()
    }

    pub fn find(&self, name: &str) -> Option<&McpServer> {
        self.servers.iter().find(|server| server.name == name)
    }

    /// Human-readable summary used by `/mcp` and `mybot mcp list`.
    pub fn describe(&self) -> String {
        if self.servers.is_empty() {
            return "no MCP servers connected".to_string();
        }
        let mut text = format!(
            "MCP servers ({} connected, {} tools)\n",
            self.servers.len(),
            self.tool_count()
        );
        for server in &self.servers {
            let names: Vec<&str> = server.tools.iter().map(|tool| tool.name.as_str()).collect();
            text.push_str(&format!(
                "  {:<14} {} tool(s): {}\n",
                server.name,
                names.len(),
                summarize(&names)
            ));
        }
        if !self.warnings.is_empty() {
            text.push_str("\nwarnings:\n");
            for warning in &self.warnings {
                text.push_str(&format!("  ! {warning}\n"));
            }
        }
        text.trim_end().to_string()
    }
}

fn summarize(names: &[&str]) -> String {
    if names.is_empty() {
        return "none".to_string();
    }
    let mut text = names.iter().take(6).copied().collect::<Vec<_>>().join(", ");
    if names.len() > 6 {
        text.push_str(&format!(", +{} more", names.len() - 6));
    }
    text
}
