use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};

const MAX_BINARY_BYTES: u64 = 200 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 1024 * 1024;

pub(super) async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    check_url(url)?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| AppError::runtime(format!("download failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::runtime(format!(
            "download returned {status}: {url}"
        )));
    }
    if response.content_length().unwrap_or(0) > MAX_BINARY_BYTES {
        return Err(AppError::runtime(format!(
            "download is larger than {} MB: {url}",
            MAX_BINARY_BYTES / 1024 / 1024
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| AppError::runtime(format!("download failed: {error}")))?;
    if bytes.len() as u64 > MAX_BINARY_BYTES {
        return Err(AppError::runtime(format!(
            "download is larger than {} MB: {url}",
            MAX_BINARY_BYTES / 1024 / 1024
        )));
    }
    Ok(bytes.to_vec())
}

pub(super) async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    check_url(url)?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| AppError::runtime(format!("download failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::runtime(format!(
            "download returned {status}: {url}"
        )));
    }
    if response.content_length().unwrap_or(0) > MAX_TEXT_BYTES {
        return Err(AppError::runtime(format!("text file is too large: {url}")));
    }
    let text = response
        .text()
        .await
        .map_err(|error| AppError::runtime(format!("download failed: {error}")))?;
    if text.len() as u64 > MAX_TEXT_BYTES {
        return Err(AppError::runtime(format!("text file is too large: {url}")));
    }
    Ok(text)
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Find `<sha256>  <asset>` (or the `*asset` binary form) in sha256sums.txt.
pub(super) fn expected_hash(sums: &str, asset: &str) -> Option<String> {
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        if name.trim_start_matches('*') == asset
            && hash.len() == 64
            && hash.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

pub(super) fn verify(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(AppError::runtime(format!(
            "checksum mismatch: expected {expected}, got {actual}"
        )))
    }
}

fn check_url(url: &str) -> Result<()> {
    if url.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or_default();
        if matches!(host, "127.0.0.1" | "localhost") {
            return Ok(());
        }
    }
    Err(AppError::runtime(format!(
        "refusing to download over an insecure URL: {url}"
    )))
}
