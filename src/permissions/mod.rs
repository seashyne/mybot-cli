pub mod bash_rules;
pub mod rules;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::{PermissionMode, Rule, RuleBehavior, RuleMatcher};
use bash_rules::BashRisk;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub tool: String,
    /// One-line summary shown in the prompt.
    pub summary: String,
    /// Command, path, or diff shown in the prompt.
    pub detail: String,
    /// Rule created when the user chooses "always allow".
    pub always_rule: Option<Rule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    AllowOnce,
    AllowAlways,
    Deny,
}

impl Decision {
    pub fn is_allowed(self) -> bool {
        !matches!(self, Decision::Deny)
    }
}

#[async_trait]
pub trait Prompter: Send + Sync {
    async fn ask(&self, request: &PermissionRequest) -> Decision;
}

/// Used by headless mode: never blocks, denies anything that needs a human.
pub struct DenyPrompter;

#[async_trait]
impl Prompter for DenyPrompter {
    async fn ask(&self, _request: &PermissionRequest) -> Decision {
        Decision::Deny
    }
}

pub struct PermissionEngine {
    mode: std::sync::RwLock<PermissionMode>,
    pub cwd: PathBuf,
    rules: Vec<Rule>,
    session_rules: Mutex<Vec<Rule>>,
}

impl PermissionEngine {
    pub fn new(mode: PermissionMode, cwd: PathBuf, rules: Vec<Rule>) -> Self {
        Self {
            mode: std::sync::RwLock::new(mode),
            cwd,
            rules,
            session_rules: Mutex::new(Vec::new()),
        }
    }

    pub fn mode(&self) -> PermissionMode {
        *self.mode.read().unwrap_or_else(|error| error.into_inner())
    }

    /// Change the mode for this process (and every sub-agent or teammate that
    /// shares the engine).
    pub fn set_mode(&self, mode: PermissionMode) {
        *self.mode.write().unwrap_or_else(|error| error.into_inner()) = mode;
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub async fn check(
        &self,
        request: &PermissionRequest,
        tool_path: Option<&Path>,
        prompter: &dyn Prompter,
    ) -> Decision {
        if self.is_hard_denied(request) {
            return Decision::Deny;
        }

        if let Some(behavior) = self.match_rule(request, tool_path).await {
            return match behavior {
                RuleBehavior::Allow => Decision::AllowOnce,
                RuleBehavior::Deny => Decision::Deny,
                RuleBehavior::Ask => self.ask(request, prompter).await,
            };
        }

        match self.mode() {
            PermissionMode::Full => Decision::AllowOnce,
            PermissionMode::Ask => self.ask(request, prompter).await,
            PermissionMode::Auto => {
                if self.auto_allows(request, tool_path) {
                    Decision::AllowOnce
                } else {
                    self.ask(request, prompter).await
                }
            }
        }
    }

    async fn ask(&self, request: &PermissionRequest, prompter: &dyn Prompter) -> Decision {
        let decision = prompter.ask(request).await;
        if decision == Decision::AllowAlways
            && let Some(rule) = &request.always_rule {
                self.session_rules.lock().await.push(rule.clone());
                let _ = rules::persist_rule(rule);
            }
        decision
    }

    async fn match_rule(
        &self,
        request: &PermissionRequest,
        tool_path: Option<&Path>,
    ) -> Option<RuleBehavior> {
        for rule in self.session_rules.lock().await.iter().rev() {
            if rules::rule_matches(rule, &request.tool, &request.detail, tool_path) {
                return Some(rule.behavior);
            }
        }
        for rule in self.rules.iter().rev() {
            if rules::rule_matches(rule, &request.tool, &request.detail, tool_path) {
                return Some(rule.behavior);
            }
        }
        None
    }

    fn auto_allows(&self, request: &PermissionRequest, tool_path: Option<&Path>) -> bool {
        match request.tool.as_str() {
            "bash" => bash_rules::classify(&request.detail) == BashRisk::Safe,
            "read" | "grep" | "glob" => tool_path.map(|p| is_inside(&self.cwd, p)).unwrap_or(true),
            "write" | "edit" => tool_path.map(|p| is_inside(&self.cwd, p)).unwrap_or(false),
            "todowrite" | "memory" => true,
            "task_create" | "task_list" | "task_update" | "send_message" | "check_messages"
            | "spawn_teammate" => true,
            _ => false,
        }
    }

    fn is_hard_denied(&self, request: &PermissionRequest) -> bool {
        if request.tool != "bash" {
            return false;
        }
        bash_rules::classify(&request.detail) == BashRisk::Dangerous
    }
}

/// True when `path` resolves inside `cwd`. Non-existent paths are resolved by
/// canonicalising the deepest existing ancestor.
pub fn is_inside(cwd: &Path, path: &Path) -> bool {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    canonicalize_lossy(&absolute).starts_with(&cwd)
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();

    loop {
        if let Ok(canonical) = existing.canonicalize() {
            let mut result = canonical;
            for part in suffix.iter().rev() {
                result.push(part);
            }
            return result;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                suffix.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Build the permission request for a tool call.
pub fn request_for(
    tool: &str,
    summary: String,
    detail: String,
    args: &serde_json::Value,
    cwd: &Path,
) -> PermissionRequest {
    let always_rule = match tool {
        "bash" => {
            let program = detail
                .split_whitespace()
                .next()
                .unwrap_or("*")
                .to_string();
            Some(rules::allow_rule(
                "bash",
                RuleMatcher::Command,
                format!("{program} *"),
            ))
        }
        "read" | "write" | "edit" | "glob" | "grep" => {
            args.get("path").and_then(|v| v.as_str()).map(|path| {
                let resolved = crate::tools::resolve_path(cwd, path);
                let parent = resolved
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| path.to_string());
                rules::allow_rule(tool, RuleMatcher::Path, format!("{parent}/*"))
            })
        }
        _ if tool.starts_with("mcp__") => {
            Some(rules::allow_rule(tool, RuleMatcher::Tool, ""))
        }
        "task_create" | "task_list" | "task_update" | "send_message" | "check_messages"
        | "spawn_teammate" => Some(rules::allow_rule(tool, RuleMatcher::Tool, "")),
        _ => None,
    };

    PermissionRequest {
        tool: tool.to_string(),
        summary,
        detail,
        always_rule,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PermissionMode, Rule};

    fn engine(mode: PermissionMode) -> PermissionEngine {
        PermissionEngine::new(mode, std::env::temp_dir(), Vec::new())
    }

    fn request(tool: &str, detail: &str) -> PermissionRequest {
        PermissionRequest {
            tool: tool.into(),
            summary: detail.into(),
            detail: detail.into(),
            always_rule: None,
        }
    }

    #[tokio::test]
    async fn auto_allows_safe_bash() {
        let decision = engine(PermissionMode::Auto)
            .check(&request("bash", "ls"), None, &DenyPrompter)
            .await;
        assert_eq!(decision, Decision::AllowOnce);
    }

    #[tokio::test]
    async fn auto_denies_risky_bash_without_prompter() {
        let decision = engine(PermissionMode::Auto)
            .check(&request("bash", "npm install"), None, &DenyPrompter)
            .await;
        assert_eq!(decision, Decision::Deny);
    }

    #[tokio::test]
    async fn hard_deny_beats_full_mode() {
        let decision = engine(PermissionMode::Full)
            .check(&request("bash", "rm -rf /"), None, &DenyPrompter)
            .await;
        assert_eq!(decision, Decision::Deny);
    }

    #[tokio::test]
    async fn allow_rule_beats_ask_mode() {
        let rules = vec![Rule {
            tool: "bash".into(),
            matcher: RuleMatcher::Command,
            pattern: "npm *".into(),
            behavior: RuleBehavior::Allow,
        }];
        let engine = PermissionEngine::new(PermissionMode::Ask, std::env::temp_dir(), rules);
        let decision = engine
            .check(&request("bash", "npm test"), None, &DenyPrompter)
            .await;
        assert_eq!(decision, Decision::AllowOnce);
    }

    #[tokio::test]
    async fn deny_rule_beats_auto() {
        let rules = vec![Rule {
            tool: "bash".into(),
            matcher: RuleMatcher::Command,
            pattern: "ls*".into(),
            behavior: RuleBehavior::Deny,
        }];
        let engine = PermissionEngine::new(PermissionMode::Auto, std::env::temp_dir(), rules);
        let decision = engine
            .check(&request("bash", "ls -la"), None, &DenyPrompter)
            .await;
        assert_eq!(decision, Decision::Deny);
    }

    #[tokio::test]
    async fn switching_the_mode_takes_effect_immediately() {
        let engine = engine(PermissionMode::Ask);
        assert_eq!(engine.mode(), PermissionMode::Ask);

        engine.set_mode(PermissionMode::Full);
        let decision = engine
            .check(&request("write", "notes.txt"), None, &DenyPrompter)
            .await;
        assert_eq!(engine.mode(), PermissionMode::Full);
        assert_eq!(decision, Decision::AllowOnce);
    }

    #[test]
    fn path_containment() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("a/b.txt");
        assert!(is_inside(dir.path(), &inside));
        assert!(!is_inside(dir.path(), Path::new("/etc/passwd")));
    }
}
