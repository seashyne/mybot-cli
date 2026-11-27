//! Git worktree isolation for background coding teammates so parallel edits
//! never collide with each other or with the lead's working copy.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct IsolatedWorktree {
    pub repo_root: PathBuf,
    pub path: PathBuf,
}

impl IsolatedWorktree {
    /// Create a detached `git worktree` at HEAD for one teammate.
    pub fn create(cwd: &Path, name: &str) -> Result<Self, String> {
        let top = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("git not available for worktree isolation: {e}"))?;
        if !top.status.success() {
            return Err("not a git repository; cannot isolate teammate worktree".into());
        }
        let repo_root = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim());
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("mybot-wt-{name}-{pid}-{nonce}"));

        let add = Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&path)
            .arg("HEAD")
            .current_dir(&repo_root)
            .output()
            .map_err(|e| format!("failed to run git worktree add: {e}"))?;
        if !add.status.success() {
            let err = String::from_utf8_lossy(&add.stderr).trim().to_string();
            return Err(format!("git worktree add failed: {err}"));
        }

        Ok(Self { repo_root, path })
    }

    /// Return a summary of uncommitted changes in this worktree, or `None` if clean.
    pub fn changes_summary(&self) -> Option<String> {
        let status = Command::new("git")
            .args(["status", "--short"])
            .current_dir(&self.path)
            .output()
            .ok()?;
        let status_text = String::from_utf8_lossy(&status.stdout).trim().to_string();
        if status_text.is_empty() {
            return None;
        }
        let mut report = format!("worktree: {}\nchanges:\n{status_text}", self.path.display());
        if let Ok(diff) = Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&self.path)
            .output()
        {
            let stat = String::from_utf8_lossy(&diff.stdout).trim().to_string();
            if !stat.is_empty() {
                report.push_str(&format!("\n{stat}"));
            }
        }
        Some(report)
    }

    /// Remove the worktree cleanly from git metadata and disk.
    pub fn remove(&self) {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .current_dir(&self.repo_root)
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_isolated_worktree_reports_changes_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q"]);
        std::fs::write(dir.path().join("hello.txt"), "v1\n").unwrap();
        git(&["add", "hello.txt"]);
        git(&["commit", "-q", "-m", "init"]);

        let wt = IsolatedWorktree::create(dir.path(), "worker").unwrap();
        assert!(wt.path.join("hello.txt").exists());
        assert!(wt.changes_summary().is_none());

        std::fs::write(wt.path.join("hello.txt"), "v2\n").unwrap();
        // Original working tree is untouched.
        assert_eq!(std::fs::read_to_string(dir.path().join("hello.txt")).unwrap(), "v1\n");
        let summary = wt.changes_summary().unwrap();
        assert!(summary.contains("hello.txt"), "got {summary}");

        wt.remove();
        assert!(!wt.path.exists());
    }
}
