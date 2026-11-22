use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::config::McpServerConfig;

pub struct HttpTransport {
    name: String,
    url: String,
    client: reqwest::Client,
    headers: HeaderMap,
    session: Mutex<Option<String>>,
    protocol: Mutex<Option<String>>,
    next_id: AtomicU64,
    timeout: Duration,
}

impl HttpTransport {
    pub fn new(name: &str, cfg: &McpServerConfig) -> Result<Self, String> {
        let url = cfg
            .url
            .clone()
            .ok_or_else(|| "missing http url".to_string())?;

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        for (key, value) in &cfg.headers {
            let name = HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| format!("invalid header name '{key}'"))?;
            let value = HeaderValue::from_str(value)
                .map_err(|_| format!("invalid header value for '{key}'"))?;
            headers.insert(name, value);
        }

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| format!("cannot build HTTP client: {error}"))?;

        Ok(Self {
            name: name.to_string(),
            url,
            client,
            headers,
            session: Mutex::new(None),
            protocol: Mutex::new(None),
            next_id: AtomicU64::new(1),
            timeout: Duration::from_secs(cfg.timeout_secs.max(1)),
        })
    }

    pub async fn set_protocol_version(&self, version: &str) {
        *self.protocol.lock().await = Some(version.to_string());
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let Some(value) = self.post(body).await? else {
            return Err(format!("{} returned an empty response", self.name));
        };
        if let Some(error) = value.get("error") {
            return Err(format_error(error));
        }
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let body = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.post(body).await.map(|_| ())
    }

    async fn post(&self, body: Value) -> Result<Option<Value>, String> {
        let id = body.get("id").and_then(Value::as_u64);
        let mut request = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .timeout(self.timeout)
            .json(&body);

        if let Some(session) = self.session.lock().await.as_ref() {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(protocol) = self.protocol.lock().await.as_ref() {
            request = request.header("MCP-Protocol-Version", protocol);
        }

        let response = request
            .send()
            .await
            .map_err(|error| format!("{} request failed: {error}", self.name))?;
        let status = response.status();

        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            *self.session.lock().await = Some(session.to_string());
        }

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {status}: {}", truncate(&text)));
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        let text = response
            .text()
            .await
            .map_err(|error| format!("{} response body failed: {error}", self.name))?;

        if content_type.contains("text/event-stream") {
            return Ok(parse_sse_response(&text, id));
        }
        if text.trim().is_empty() {
            return Ok(None);
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("{} returned invalid JSON: {error}", self.name))?;
        Ok(Some(value))
    }
}

fn parse_sse_response(text: &str, id: Option<u64>) -> Option<Value> {
    let mut found = None;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if id.is_none() || value.get("id").and_then(Value::as_u64) == id {
            found = Some(value);
        }
    }
    found
}

fn format_error(error: &Value) -> String {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("JSON-RPC error");
    match error.get("code").and_then(Value::as_i64) {
        Some(code) => format!("{message} (code {code})"),
        None => message.to_string(),
    }
}

fn truncate(text: &str) -> String {
    let text = text.trim();
    if text.len() > 300 {
        format!("{}…", &text[..300])
    } else {
        text.to_string()
    }
}
