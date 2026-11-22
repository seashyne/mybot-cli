use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::LOCATION;
use serde_json::{Value, json};

use super::truncate::head_tail;
use super::{ExecutionMode, Tool, ToolCtx, ToolOutput, arg_str, arg_u64};

pub struct WebFetchTool;

const DEFAULT_TIMEOUT_SECS: u64 = 20;
const MAX_DOWNLOAD_BYTES: usize = 512 * 1024;
const DEFAULT_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_REDIRECTS: usize = 5;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web_fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch a public http/https URL and return readable text (HTML tags stripped). Blocks localhost and private network addresses."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Public http:// or https:// URL to fetch"
                },
                "max_bytes": {
                    "type": "integer",
                    "description": "Maximum output bytes to return (default 32768, max 131072)"
                }
            },
            "required": ["url"]
        })
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    fn summary(&self, args: &Value) -> String {
        let url = arg_str(args, "url").unwrap_or_default();
        format!("web_fetch {url}")
    }

    async fn execute(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(raw_url) = arg_str(args, "url") else {
            return ToolOutput::error("missing required argument: url");
        };
        let max_output = arg_u64(args, "max_bytes")
            .map(|n| (n as usize).clamp(512, 128 * 1024))
            .unwrap_or(DEFAULT_OUTPUT_BYTES);

        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("mybot/", env!("CARGO_PKG_VERSION")))
            .build()
        {
            Ok(client) => client,
            Err(error) => return ToolOutput::error(format!("cannot create HTTP client: {error}")),
        };

        let mut current = match parse_and_validate_url(raw_url).await {
            Ok(url) => url,
            Err(message) => return ToolOutput::error(message),
        };

        let response = {
            let mut redirects = 0usize;
            loop {
                let request = client.get(current.clone()).send();
                let resp = tokio::select! {
                    _ = ctx.cancel.cancelled() => return ToolOutput::error("request cancelled"),
                    result = request => match result {
                        Ok(resp) => resp,
                        Err(error) => return ToolOutput::error(format!("fetch failed: {error}")),
                    }
                };

                if resp.status().is_redirection()
                    && let Some(loc) = resp.headers().get(LOCATION).and_then(|v| v.to_str().ok())
                {
                    if redirects >= MAX_REDIRECTS {
                        return ToolOutput::error("too many redirects");
                    }
                    let joined = match current.join(loc) {
                        Ok(url) => url,
                        Err(error) => return ToolOutput::error(format!("invalid redirect: {error}")),
                    };
                    current = match parse_and_validate_url(joined.as_str()).await {
                        Ok(url) => url,
                        Err(message) => return ToolOutput::error(message),
                    };
                    redirects += 1;
                    continue;
                }
                break resp;
            }
        };

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = tokio::select! {
            _ = ctx.cancel.cancelled() => return ToolOutput::error("request cancelled"),
            item = stream.next() => item,
        } {
            let Ok(chunk) = chunk else {
                return ToolOutput::error("error reading response body");
            };
            let take = (MAX_DOWNLOAD_BYTES - bytes.len()).min(chunk.len());
            bytes.extend_from_slice(&chunk[..take]);
            if bytes.len() >= MAX_DOWNLOAD_BYTES {
                break;
            }
        }

        let raw_text = String::from_utf8_lossy(&bytes).to_string();
        let cleaned = if content_type.contains("html") {
            strip_html(&raw_text)
        } else {
            raw_text
        };
        let header = format!("URL: {current}\nStatus: {status}\n\n");
        let (body, _) = head_tail(&cleaned, max_output);
        if !status.is_success() {
            return ToolOutput::error(format!("{header}{body}"));
        }
        ToolOutput::ok(format!("{header}{body}"))
    }
}

async fn parse_and_validate_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|e| format!("invalid URL: {e}"))?;
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(format!("unsupported URL scheme '{other}' (use http or https)")),
    }
    let host = url
        .host_str()
        .ok_or_else(|| "URL must include a host".to_string())?;
    let lower = host.to_ascii_lowercase();
    if lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
    {
        return Err(format!("blocked private/local host '{host}'"));
    }

    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            return Err(format!("blocked non-public IP address '{ip}'"));
        }
        return Ok(url);
    }

    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("cannot resolve '{host}': {e}"))?;
    let mut saw_any = false;
    for addr in addrs {
        saw_any = true;
        if is_blocked_ip(addr.ip()) {
            return Err(format!("blocked non-public IP address '{}'", addr.ip()));
        }
    }
    if !saw_any {
        return Err(format!("cannot resolve '{host}'"));
    }
    Ok(url)
}

pub(crate) fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_v4(mapped);
            }
            is_blocked_v6(v6)
        }
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        || (ip.segments()[0] & 0xffc0) == 0xfe80
}

pub(crate) fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_tag: Option<&'static str> = None;
    let mut tag_buf = String::new();

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let lower = tag_buf.trim().to_ascii_lowercase();
                if let Some(end_tag) = skip_tag {
                    if lower.starts_with(&format!("/{end_tag}")) {
                        skip_tag = None;
                    }
                } else if lower.starts_with("script") {
                    skip_tag = Some("script");
                } else if lower.starts_with("style") {
                    skip_tag = Some("style");
                } else if matches!(
                    lower.split_whitespace().next().unwrap_or(""),
                    "p" | "/p" | "br" | "br/" | "div" | "/div" | "li" | "h1" | "h2" | "h3" | "tr"
                ) {
                    out.push('\n');
                }
                tag_buf.clear();
            } else if tag_buf.len() < 64 {
                tag_buf.push(ch);
            }
            continue;
        }
        if ch == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }
        if skip_tag.is_none() {
            out.push(ch);
        }
    }

    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback_private_and_metadata_ips() {
        assert!(is_blocked_ip("127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("10.0.0.5".parse().unwrap()));
        assert!(is_blocked_ip("172.16.0.1".parse().unwrap()));
        assert!(is_blocked_ip("192.168.1.1".parse().unwrap()));
        assert!(is_blocked_ip("169.254.169.254".parse().unwrap()));
        assert!(is_blocked_ip("::1".parse().unwrap()));
        assert!(is_blocked_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_blocked_ip("93.184.216.34".parse().unwrap()));
    }

    #[test]
    fn strips_scripts_styles_and_tags() {
        let html = "<html><head><style>body{}</style><script>alert(1)</script></head>\
                    <body><h1>Hello</h1><p>World</p></body></html>";
        let text = strip_html(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("alert(1)"));
        assert!(!text.contains("body{}"));
    }
}
