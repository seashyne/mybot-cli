use serde_json::{Value, json};

use super::config::McpServerConfig;
use super::http::HttpTransport;
use super::stdio::StdioTransport;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpToolDef {
    fn from_value(value: Value) -> Option<Self> {
        let name = value.get("name").and_then(Value::as_str)?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        let description = value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| format!("MCP tool '{name}'"));
        let input_schema = value
            .get("inputSchema")
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({"type": "object"}));
        Some(Self {
            name,
            description,
            input_schema,
        })
    }
}

#[derive(Debug, Clone)]
pub struct McpCallResult {
    pub content: String,
    pub is_error: bool,
}

impl McpCallResult {
    fn from_value(value: Value) -> Self {
        let is_error = value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut parts = Vec::new();
        if let Some(items) = value.get("content").and_then(Value::as_array) {
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => parts.push(
                        item.get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    ),
                    _ => parts.push(item.to_string()),
                }
            }
        } else {
            parts.push(value.to_string());
        }
        Self {
            content: parts.join("\n"),
            is_error,
        }
    }
}

enum Transport {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

impl Transport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        match self {
            Self::Stdio(transport) => transport.request(method, params).await,
            Self::Http(transport) => transport.request(method, params).await,
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        match self {
            Self::Stdio(transport) => transport.notify(method, params).await,
            Self::Http(transport) => transport.notify(method, params).await,
        }
    }

    async fn set_protocol_version(&self, version: &str) {
        if let Self::Http(transport) = self {
            transport.set_protocol_version(version).await;
        }
    }
}

pub struct Client {
    name: String,
    transport: Transport,
}

impl Client {
    pub async fn connect(name: &str, cfg: &McpServerConfig) -> Result<Self, String> {
        cfg.validate()?;
        let transport = if cfg.command.is_some() {
            Transport::Stdio(StdioTransport::spawn(name, cfg).await?)
        } else {
            Transport::Http(HttpTransport::new(name, cfg)?)
        };
        let client = Self {
            name: name.to_string(),
            transport,
        };
        client.initialize().await?;
        Ok(client)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    async fn initialize(&self) -> Result<(), String> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "mybot",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });
        let result = self.transport.request("initialize", params).await?;
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(PROTOCOL_VERSION);
        self.transport.set_protocol_version(version).await;
        self.transport
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, String> {
        let result = self.transport.request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tools.into_iter().filter_map(McpToolDef::from_value).collect())
    }

    pub async fn call_tool(&self, tool: &str, arguments: Value) -> Result<McpCallResult, String> {
        let result = self
            .transport
            .request(
                "tools/call",
                json!({"name": tool, "arguments": arguments}),
            )
            .await?;
        Ok(McpCallResult::from_value(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_definitions() {
        let value = json!({
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}
        });
        let tool = McpToolDef::from_value(value).unwrap();
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.description, "Read a file");
        assert_eq!(tool.input_schema["properties"]["path"]["type"], "string");
    }

    #[test]
    fn defaults_schema_and_description() {
        let tool = McpToolDef::from_value(json!({"name": "ping"})).unwrap();
        assert!(tool.description.contains("ping"));
        assert_eq!(tool.input_schema["type"], "object");
    }

    #[test]
    fn flattens_text_content_and_errors() {
        let result = McpCallResult::from_value(json!({
            "isError": true,
            "content": [
                {"type": "text", "text": "line one"},
                {"type": "image", "data": "abc"}
            ]
        }));
        assert!(result.is_error);
        assert!(result.content.contains("line one"));
        assert!(result.content.contains("\"type\":\"image\""));
    }
}
