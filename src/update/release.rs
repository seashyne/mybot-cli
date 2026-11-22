use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::version;

const DEFAULT_REPO: &str = "seashyne/mybot-cli";
const DEFAULT_API: &str = "https://api.github.com";

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub version: semver::Version,
    pub url: String,
    pub published_at: Option<String>,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }

    pub fn asset_names(&self) -> Vec<&str> {
        self.assets.iter().map(|asset| asset.name.as_str()).collect()
    }
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// Fetch the latest release, or a specific tag, from the GitHub API.
pub async fn fetch(tag: Option<&str>) -> Result<Release> {
    let repo = std::env::var("MYBOT_UPDATE_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let api = std::env::var("MYBOT_UPDATE_API").unwrap_or_else(|_| DEFAULT_API.to_string());
    let client = reqwest::Client::builder()
        .user_agent(format!("mybot/{}", version::VERSION))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| AppError::runtime(format!("cannot build http client: {error}")))?;

    let candidates = tag_candidates(tag);
    for candidate in &candidates {
        let url = api_url(&api, &repo, candidate.as_deref());
        if let Some(release) = get_release(&client, &url).await? {
            return Ok(release);
        }
    }

    match tag {
        Some(tag) => Err(AppError::runtime(format!(
            "no release found for tag '{tag}' in {repo}"
        ))),
        None => Err(AppError::runtime(format!(
            "no published release found in {repo}"
        ))),
    }
}

fn tag_candidates(tag: Option<&str>) -> Vec<Option<String>> {
    match tag {
        None => vec![None],
        Some(tag) => {
            let tag = tag.trim();
            let mut out = vec![Some(tag.to_string())];
            if !tag.starts_with('v') {
                out.push(Some(format!("v{tag}")));
            }
            out
        }
    }
}

fn api_url(api: &str, repo: &str, tag: Option<&str>) -> String {
    let base = api.trim_end_matches('/');
    match tag {
        Some(tag) => format!("{base}/repos/{repo}/releases/tags/{tag}"),
        None => format!("{base}/repos/{repo}/releases/latest"),
    }
}

async fn get_release(client: &reqwest::Client, url: &str) -> Result<Option<Release>> {
    let mut request = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Ok(token) = std::env::var("MYBOT_GITHUB_TOKEN")
        && !token.trim().is_empty() {
            request = request.bearer_auth(token.trim());
        }

    let response = request
        .send()
        .await
        .map_err(|error| AppError::runtime(format!("cannot reach the GitHub API: {error}")))?;
    let status = response.status();

    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(AppError::runtime(
            "GitHub API rate limit reached; set MYBOT_GITHUB_TOKEN or retry later",
        ));
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::runtime(format!(
            "GitHub API returned {status}: {}",
            body.trim()
        )));
    }

    let text = response
        .text()
        .await
        .map_err(|error| AppError::runtime(format!("cannot read release response: {error}")))?;
    Ok(Some(from_json(&text)?))
}

pub(super) fn from_json(text: &str) -> Result<Release> {
    let api: ApiRelease = serde_json::from_str(text)
        .map_err(|error| AppError::runtime(format!("invalid release response: {error}")))?;
    let version = parse_tag(&api.tag_name)?;
    Ok(Release {
        tag: api.tag_name,
        version,
        url: api.html_url,
        published_at: api.published_at,
        assets: api.assets,
    })
}

pub(super) fn parse_tag(tag: &str) -> Result<semver::Version> {
    let raw = tag.trim().trim_start_matches('v');
    semver::Version::parse(raw).map_err(|error| {
        AppError::runtime(format!(
            "release tag '{tag}' is not a semantic version: {error}"
        ))
    })
}
