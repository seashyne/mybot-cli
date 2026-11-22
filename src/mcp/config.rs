use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One MCP server entry from `mcp.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Command for a stdio server.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// URL for a streamable HTTP server.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    60
}

impl McpServerConfig {
    pub fn validate(&self) -> Result<(), String> {
        match (&self.command, &self.url) {
            (Some(command), None) if command.trim().is_empty() => {
                Err("command is empty".to_string())
            }
            (Some(_), None) => Ok(()),
            (None, Some(url)) if !(url.starts_with("http://") || url.starts_with("https://")) => {
                Err(format!("url must start with http:// or https://: {url}"))
            }
            (None, Some(_)) => Ok(()),
            (Some(_), Some(_)) => Err("set either command or url, not both".to_string()),
            (None, None) => Err("missing command (stdio) or url (http)".to_string()),
        }
    }

    pub fn transport_label(&self) -> String {
        match (&self.command, &self.url) {
            (Some(command), _) => {
                let args = if self.args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", self.args.join(" "))
                };
                format!("stdio: {command}{args}")
            }
            (None, Some(url)) => format!("http: {url}"),
            _ => "invalid".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl McpConfig {
    /// Load the user file, then let the project file override entries by name.
    /// Parse failures are reported as warnings instead of aborting startup.
    pub fn load(cwd: &Path) -> (Self, Vec<String>) {
        let mut config = Self::default();
        let mut warnings = Vec::new();
        for path in Self::paths(cwd) {
            if !path.exists() {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => match toml::from_str::<Self>(&text) {
                    Ok(parsed) => {
                        for (name, server) in parsed.servers {
                            config.servers.insert(name, server);
                        }
                    }
                    Err(error) => warnings.push(format!(
                        "ignoring {}: {error}",
                        path.display()
                    )),
                },
                Err(error) => warnings.push(format!(
                    "cannot read {}: {error}",
                    path.display()
                )),
            }
        }
        (config, warnings)
    }

    /// User config first, project config second (project wins).
    pub fn paths(cwd: &Path) -> Vec<PathBuf> {
        vec![
            crate::config::data_dir().join("mcp.toml"),
            cwd.join(".mybot/mcp.toml"),
        ]
    }

    pub fn enabled(&self) -> impl Iterator<Item = (&str, &McpServerConfig)> {
        self.servers
            .iter()
            .filter(|(_, server)| server.enabled)
            .map(|(name, server)| (name.as_str(), server))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stdio_and_http_entries() {
        let text = r#"
[servers.files]
command = "npx"
args = ["-y", "server-filesystem", "."]
env = { DEBUG = "0" }

[servers.remote]
url = "https://example.com/mcp"
headers = { Authorization = "Bearer token" }
timeout_secs = 10
"#;
        let config: McpConfig = toml::from_str(text).unwrap();
        assert_eq!(config.servers.len(), 2);
        let files = &config.servers["files"];
        assert!(files.enabled);
        assert_eq!(files.timeout_secs, 60);
        assert_eq!(files.args.len(), 3);
        let remote = &config.servers["remote"];
        assert_eq!(remote.timeout_secs, 10);
        assert_eq!(remote.headers["Authorization"], "Bearer token");
    }

    #[test]
    fn validates_transport_shape() {
        let mut server = McpServerConfig {
            command: Some("echo".into()),
            url: None,
            args: vec![],
            env: BTreeMap::new(),
            headers: BTreeMap::new(),
            enabled: true,
            timeout_secs: 60,
        };
        assert!(server.validate().is_ok());
        server.url = Some("https://example.com".into());
        assert!(server.validate().is_err());
        server.command = None;
        assert!(server.validate().is_ok());
        server.url = Some("ftp://example.com".into());
        assert!(server.validate().is_err());
    }

    #[test]
    fn disabled_entries_are_skipped() {
        let config: McpConfig = toml::from_str(
            "[servers.a]\ncommand = \"x\"\nenabled = false\n[servers.b]\ncommand = \"y\"\n",
        )
        .unwrap();
        let names: Vec<&str> = config.enabled().map(|(name, _)| name).collect();
        assert_eq!(names, vec!["b"]);
    }
}
