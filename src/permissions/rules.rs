use std::path::Path;

use crate::config::{Rule, RuleBehavior, RuleMatcher};

pub fn rule_matches(rule: &Rule, tool: &str, detail: &str, path: Option<&Path>) -> bool {
    if !wildcard_matches(&rule.tool, tool) {
        return false;
    }
    match rule.matcher {
        RuleMatcher::Tool => true,
        RuleMatcher::Command => wildcard_matches(&rule.pattern, detail),
        RuleMatcher::Path => {
            let Some(path) = path else {
                return false;
            };
            let path = path.to_string_lossy();
            wildcard_matches(&rule.pattern, &path) || path.contains(rule.pattern.trim_end_matches("**"))
        }
    }
}

/// Minimal `*` wildcard matcher (no external dependency, no regex escaping).
pub fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);

    while v < value.len() {
        if p < pattern.len() && (pattern[p] == value[v] || pattern[p] == b'?') {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            mark = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            mark += 1;
            v = mark;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// Persist an "always allow" rule into `~/.mybot/config.toml`.
pub fn persist_rule(rule: &Rule) -> crate::error::Result<()> {
    let path = crate::config::config_path();
    let mut config: crate::config::Config = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text)
            .map_err(|error| crate::error::AppError::config(format!("invalid config: {error}")))?
    } else {
        crate::config::Config::default()
    };

    config.permissions.rules.push(rule.clone());

    let text = toml::to_string_pretty(&config)
        .map_err(|error| crate::error::AppError::config(format!("cannot serialise config: {error}")))?;
    crate::config::schema::write_restricted(&path, &text)?;
    Ok(())
}

pub fn allow_rule(tool: &str, matcher: RuleMatcher, pattern: impl Into<String>) -> Rule {
    Rule {
        tool: tool.to_string(),
        matcher,
        pattern: pattern.into(),
        behavior: RuleBehavior::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_basics() {
        assert!(wildcard_matches("git *", "git status"));
        assert!(wildcard_matches("*", "anything"));
        assert!(wildcard_matches("*.rs", "main.rs"));
        assert!(!wildcard_matches("*.rs", "main.py"));
        assert!(wildcard_matches("a?c", "abc"));
        assert!(!wildcard_matches("abc", "abcd"));
    }

    #[test]
    fn command_rule_matches() {
        let rule = allow_rule("bash", RuleMatcher::Command, "git status*");
        assert!(rule_matches(&rule, "bash", "git status --short", None));
        assert!(!rule_matches(&rule, "bash", "git push", None));
    }

    #[test]
    fn tool_rule_matches_any_args() {
        let rule = allow_rule("read", RuleMatcher::Tool, "");
        assert!(rule_matches(&rule, "read", "read src/main.rs", None));
        assert!(!rule_matches(&rule, "bash", "ls", None));
    }
}
