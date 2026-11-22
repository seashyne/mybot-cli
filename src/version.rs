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
}
