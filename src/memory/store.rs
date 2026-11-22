use chrono::Utc;

use crate::error::{AppError, Result};

use super::{MAX_ENTRY_BYTES, MAX_PROMPT_BYTES, Memory, MemoryKind, MemoryStore, cap_bytes};

impl MemoryStore {
    /// Create or replace one memory and refresh the index.
    pub fn write(
        &mut self,
        name: &str,
        title: &str,
        description: &str,
        kind: MemoryKind,
        body: &str,
    ) -> Result<std::path::PathBuf> {
        let name = super::slugify(name);
        if name.is_empty() {
            return Err(AppError::usage(
                "memory name must contain at least one letter or digit",
            ));
        }
        if body.len() > MAX_ENTRY_BYTES {
            return Err(AppError::usage(format!(
                "memory body is too large ({} bytes, limit {MAX_ENTRY_BYTES})",
                body.len()
            )));
        }

        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join(format!("{name}.md"));
        let title = if title.trim().is_empty() {
            name.replace('-', " ")
        } else {
            title.trim().to_string()
        };
        let description = if description.trim().is_empty() {
            body.lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(120)
                .collect()
        } else {
            description.trim().to_string()
        };
        let updated = Utc::now();

        let mut text = String::from("---\n");
        text.push_str(&format!("name: {name}\n"));
        text.push_str(&format!("title: {title}\n"));
        text.push_str(&format!("description: {description}\n"));
        text.push_str(&format!("type: {}\n", kind.as_str()));
        text.push_str(&format!("updated: {}\n", updated.to_rfc3339()));
        text.push_str("---\n\n");
        text.push_str(body.trim_end());
        text.push('\n');
        std::fs::write(&path, text)?;

        let memory = Memory {
            name,
            title,
            description,
            kind,
            body: body.trim().to_string(),
            path: path.clone(),
            updated: Some(updated),
        };
        self.entries.retain(|entry| entry.name != memory.name);
        self.entries.push(memory);
        self.entries.sort_by(|a, b| {
            a.kind
                .order()
                .cmp(&b.kind.order())
                .then_with(|| a.name.cmp(&b.name))
        });
        self.save_index()?;
        Ok(path)
    }

    pub fn delete(&mut self, name: &str) -> Result<bool> {
        let name = super::slugify(name);
        let Some(index) = self.entries.iter().position(|entry| entry.name == name) else {
            return Ok(false);
        };
        let memory = self.entries.remove(index);
        if memory.path.exists() {
            std::fs::remove_file(&memory.path)?;
        }
        self.save_index()?;
        Ok(true)
    }

    pub fn render_index(&self) -> String {
        let mut text = String::from("# Memory\n");
        if self.entries.is_empty() {
            text.push_str("\n(no entries yet)\n");
            return text;
        }
        text.push('\n');
        for entry in &self.entries {
            text.push_str(&format!(
                "- [{}]({}.md) — {} ({})\n",
                entry.title,
                entry.name,
                entry.description,
                entry.kind.as_str()
            ));
        }
        text
    }

    pub fn save_index(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(self.index_path(), self.render_index())?;
        Ok(())
    }

    /// System-prompt section. Returns `None` when the project has no memories.
    pub fn prompt_block(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let mut block = String::from(
            "<memory>\nDurable notes about this project, carried over from earlier sessions.\nUse the `memory` tool to read an entry or save a new one.\n\n",
        );
        block.push_str(&self.render_index());
        block.push_str("</memory>");
        Some(cap_bytes(block, MAX_PROMPT_BYTES))
    }
}
