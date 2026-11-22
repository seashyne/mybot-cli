//! Persistent project memory.
//!
//! Each project gets a directory under `~/.mybot/memory/<project>-<hash>` with
//! one markdown file per memory and a `MEMORY.md` index. The index is injected
//! into the system prompt; the `memory` tool reads and updates the entries.
//!
//! Frontmatter follows the same simple `key: value` format as skills:
//!
//! ```text
//! ---
//! name: auth-decision
//! title: Auth uses signed cookies
//! description: Why the session cookie is signed instead of stored server-side
//! type: project
//! updated: 2026-11-22T10:00:00Z
//! ---
//! The session cookie is signed because ...
//! ```

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_ENTRY_BYTES: usize = 64 * 1024;
const MAX_PROMPT_BYTES: usize = 8 * 1024;
const INDEX_FILE: &str = "MEMORY.md";

mod store;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    User,
    Feedback,
    Project,
    Reference,
}

impl MemoryKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "user" => Some(Self::User),
            "feedback" => Some(Self::Feedback),
            "project" => Some(Self::Project),
            "reference" => Some(Self::Reference),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
        }
    }

    fn order(self) -> u8 {
        match self {
            Self::Project => 0,
            Self::Feedback => 1,
            Self::User => 2,
            Self::Reference => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Memory {
    pub name: String,
    pub title: String,
    pub description: String,
    pub kind: MemoryKind,
    pub body: String,
    pub path: PathBuf,
    pub updated: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MemoryStore {
    pub root: PathBuf,
    pub entries: Vec<Memory>,
}

impl MemoryStore {
    /// Memory directory for a working directory.
    pub fn root_for(cwd: &Path) -> PathBuf {
        let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let base = canonical
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "project".to_string());
        let base = slugify(&base);
        let mut hasher = DefaultHasher::new();
        canonical.to_string_lossy().hash(&mut hasher);
        let hash = hasher.finish();
        crate::config::data_dir()
            .join("memory")
            .join(format!("{base}-{hash:016x}"))
    }

    pub fn load(cwd: &Path) -> Self {
        Self::load_from(Self::root_for(cwd))
    }

    pub fn load_from(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut entries = Vec::new();
        if let Ok(dir) = std::fs::read_dir(&root) {
            for entry in dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("md") {
                    continue;
                }
                if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE) {
                    continue;
                }
                if let Some(memory) = read_memory(&path) {
                    entries.push(memory);
                }
            }
        }
        entries.sort_by(|a, b| {
            a.kind
                .order()
                .cmp(&b.kind.order())
                .then_with(|| a.name.cmp(&b.name))
        });
        Self { root, entries }
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    pub fn get(&self, name: &str) -> Option<&Memory> {
        let name = slugify(name);
        self.entries.iter().find(|entry| entry.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn read_memory(path: &Path) -> Option<Memory> {
    let text = std::fs::read_to_string(path).ok()?;
    let (meta, body) = crate::skills::split_frontmatter(&text);
    let fallback = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = meta
        .get("name")
        .map(|value| slugify(value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| slugify(&fallback));
    if name.is_empty() {
        return None;
    }
    let title = meta
        .get("title")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| name.replace('-', " "));
    let description = meta
        .get("description")
        .cloned()
        .unwrap_or_else(|| {
            body.lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(120)
                .collect()
        });
    let kind = meta
        .get("type")
        .and_then(|value| MemoryKind::parse(value))
        .unwrap_or(MemoryKind::Project);
    let updated = meta
        .get("updated")
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));

    Some(Memory {
        name,
        title,
        description,
        kind,
        body: body.trim().to_string(),
        path: path.to_path_buf(),
        updated,
    })
}

/// Lowercase ASCII slug: letters, digits, `-` and `_`.
pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= 64 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

fn cap_bytes(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[memory index truncated]");
    text
}
