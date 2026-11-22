use std::path::PathBuf;

/// Cargo package version, for example `0.1.0`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Short git commit captured by `build.rs`, or `unknown`.
pub const GIT_SHA: &str = env!("MYBOT_GIT_SHA");
/// Commit date (`YYYY-MM-DD`) captured by `build.rs`, or `unknown`.
pub const GIT_DATE: &str = env!("MYBOT_GIT_DATE");
/// Build target triple, for example `x86_64-unknown-linux-gnu`.
pub const TARGET: &str = env!("MYBOT_TARGET");
/// One-line version used by `--version`.
pub const SHORT: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("MYBOT_GIT_SHA"), ")");
/// Embedded release notes displayed by `/changelog` and the welcome card.
pub const CHANGELOG: &str = include_str!("../CHANGELOG.md");

/// Extract compact one-line bullet highlights from the latest release section.
pub fn latest_changelog_highlights(limit: usize) -> Vec<String> {
    let mut in_latest = false;
    let mut items: Vec<String> = Vec::new();

    for line in CHANGELOG.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## [") {
            if trimmed.starts_with("## [Unreleased]") {
                continue;
            }
            if in_latest {
                break;
            }
            in_latest = true;
            continue;
        }
        if !in_latest {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            if items.len() >= limit {
                break;
            }
            items.push(rest.replace("**", ""));
        } else if line.starts_with("  ")
            && !trimmed.is_empty()
            && let Some(last) = items.last_mut()
        {
            last.push(' ');
            last.push_str(&trimmed.replace("**", ""));
        }
    }

    items
}

pub fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub fn binary_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Multi-line report printed by `mybot version`.
pub fn long() -> String {
    let mut text = format!("mybot {SHORT}\n");
    text.push_str(&format!("target:  {TARGET}\n"));
    text.push_str(&format!("commit:  {GIT_SHA} ({GIT_DATE})\n"));
    text.push_str(&format!("profile: {}\n", profile()));
    if let Some(path) = binary_path() {
        text.push_str(&format!("binary:  {}\n", path.display()));
    }
    text.push_str(&format!("config:  {}", crate::config::data_dir().display()));
    text
}

pub fn json() -> serde_json::Value {
    serde_json::json!({
        "name": "mybot",
        "version": VERSION,
        "commit": GIT_SHA,
        "commitDate": GIT_DATE,
        "target": TARGET,
        "profile": profile(),
        "binary": binary_path().map(|path| path.display().to_string()),
        "config": crate::config::data_dir().display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_version_contains_package_version() {
        assert!(SHORT.starts_with(VERSION));
    }

    #[test]
    fn json_exposes_the_build_identity() {
        let value = json();
        assert_eq!(value["name"], "mybot");
        assert_eq!(value["version"], VERSION);
        assert_eq!(value["target"], TARGET);
    }

    #[test]
    fn extracts_latest_changelog_highlights() {
        let items = latest_changelog_highlights(2);
        assert_eq!(items.len(), 2);
        assert!(!items[0].contains("**"));
    }
}
