use std::path::Path;

const MAX_CONTEXT_BYTES: usize = 32 * 1024;

/// Project instructions: `CLAUDE.md` first, then `AGENTS.md`, workspace root only.
pub fn project_context(cwd: &Path) -> Option<String> {
    for name in ["CLAUDE.md", "AGENTS.md"] {
        let path = cwd.join(name);
        if let Some(text) = read_capped(&path) {
            return Some(format!(
                "<project-instructions source=\"{name}\">\n{text}\n</project-instructions>"
            ));
        }
    }
    None
}

/// Global instructions from `~/.mybot/AGENTS.md`.
pub fn global_context() -> Option<String> {
    let path = crate::config::data_dir().join("AGENTS.md");
    read_capped(&path).map(|text| {
        format!("<global-instructions>\n{text}\n</global-instructions>")
    })
}

fn read_capped(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let truncated = bytes.len() > MAX_CONTEXT_BYTES;
    let end = if truncated {
        floor_boundary(&bytes, MAX_CONTEXT_BYTES)
    } else {
        bytes.len()
    };
    let mut text = String::from_utf8_lossy(&bytes[..end]).to_string();
    if truncated {
        text.push_str("\n[truncated at 32 KB]");
    }
    Some(text)
}

fn floor_boundary(bytes: &[u8], mut index: usize) -> usize {
    while index > 0 && (bytes[index] & 0b1100_0000) == 0b1000_0000 {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "use tabs").unwrap();
        let context = project_context(dir.path()).unwrap();
        assert!(context.contains("use tabs"));
        assert!(context.contains("AGENTS.md"));
    }

    #[test]
    fn claude_md_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "claude").unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "agents").unwrap();
        let context = project_context(dir.path()).unwrap();
        assert!(context.contains("claude"));
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(project_context(dir.path()).is_none());
    }
}
