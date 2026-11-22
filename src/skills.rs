//! Skills: reusable instruction packs the model can load on demand.
//!
//! A skill is a directory with a `SKILL.md`:
//!
//! ```text
//! ---
//! name: rust-review
//! description: Review Rust code for correctness and idioms
//! ---
//! # Rust review
//! 1. Read the diff…
//! ```
//!
//! Search order (later wins on a name clash):
//! `~/.mybot/skills`, `~/.mybot/plugins/*/skills`,
//! `<cwd>/.mybot/skills`, `<cwd>/.mybot/plugins/*/skills`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    User,
    Project,
    Plugin(String),
}

impl SkillSource {
    pub fn label(&self) -> String {
        match self {
            SkillSource::User => "user".into(),
            SkillSource::Project => "project".into(),
            SkillSource::Plugin(name) => format!("plugin:{name}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
    pub source: SkillSource,
}

#[derive(Debug, Clone, Default)]
pub struct SkillSet {
    pub skills: Vec<Skill>,
}

impl SkillSet {
    pub fn discover(cwd: &Path) -> Self {
        let mut roots: Vec<(PathBuf, SkillSource)> = Vec::new();
        let plugin_roots = crate::plugins::roots(cwd);

        if let Some(home) = dirs::home_dir() {
            roots.push((home.join(".mybot/skills"), SkillSource::User));
        }
        for (root, project) in &plugin_roots {
            if !project {
                roots.push((root.join("skills"), SkillSource::Plugin(plugin_name(root))));
            }
        }

        roots.push((cwd.join(".mybot/skills"), SkillSource::Project));
        for (root, project) in &plugin_roots {
            if *project {
                roots.push((root.join("skills"), SkillSource::Plugin(plugin_name(root))));
            }
        }

        Self::from_roots(&roots)
    }

    pub fn from_roots(roots: &[(PathBuf, SkillSource)]) -> Self {
        let mut skills: Vec<Skill> = Vec::new();
        for (root, source) in roots {
            for skill in load_dir(root, source.clone()) {
                skills.retain(|existing| existing.name != skill.name);
                skills.push(skill);
            }
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Self { skills }
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn names(&self) -> Vec<&str> {
        self.skills.iter().map(|skill| skill.name.as_str()).collect()
    }

    /// System-prompt section that tells the model which skills exist.
    pub fn prompt_block(&self) -> Option<String> {
        if self.skills.is_empty() {
            return None;
        }
        let mut block = String::from(
            "<skills>\nLoad a skill with the `skill` tool before following its instructions.\n",
        );
        for skill in &self.skills {
            block.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }
        block.push_str("</skills>");
        Some(block)
    }
}

fn plugin_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn load_dir(dir: &Path, source: SkillSource) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| load_skill(&path, source.clone()))
        .collect()
}

fn load_skill(dir: &Path, source: SkillSource) -> Option<Skill> {
    let path = dir.join("SKILL.md");
    let text = std::fs::read_to_string(&path).ok()?;
    let (meta, body) = split_frontmatter(&text);

    let fallback = dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = meta
        .get("name")
        .filter(|name| is_valid_name(name))
        .cloned()
        .unwrap_or(fallback);
    if name.is_empty() {
        return None;
    }

    let description = meta.get("description").cloned().unwrap_or_else(|| {
        body.lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim_start_matches('#')
            .trim()
            .to_string()
    });

    Some(Skill {
        name,
        description,
        body,
        path,
        source,
    })
}

pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

/// Split an optional `---` frontmatter block from the body.
pub fn split_frontmatter(text: &str) -> (BTreeMap<String, String>, String) {
    let normalized = text.replace("\r\n", "\n");
    let trimmed = normalized.trim_start();
    let Some(rest) = trimmed.strip_prefix("---\n") else {
        return (BTreeMap::new(), normalized);
    };
    let Some(end) = rest.find("\n---") else {
        return (BTreeMap::new(), normalized);
    };

    let header = &rest[..end];
    let body = rest[end + 4..].trim_start_matches('\n').to_string();

    let mut meta = BTreeMap::new();
    for line in header.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            meta.insert(key.trim().to_string(), value.to_string());
        }
    }
    (meta, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn skill_md(name: &str, description: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}")
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let (meta, body) = split_frontmatter("---\nname: demo\ndescription: hi\n---\n# Title\nbody");
        assert_eq!(meta.get("name").unwrap(), "demo");
        assert_eq!(meta.get("description").unwrap(), "hi");
        assert_eq!(body, "# Title\nbody");
    }

    #[test]
    fn text_without_frontmatter_is_all_body() {
        let (meta, body) = split_frontmatter("# Just markdown\n");
        assert!(meta.is_empty());
        assert_eq!(body, "# Just markdown\n");
    }

    #[test]
    fn project_skills_override_user_skills() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user");
        let project = dir.path().join("project");
        write(
            &user.join("review/SKILL.md"),
            &skill_md("review", "user version", "user body"),
        );
        write(
            &project.join("review/SKILL.md"),
            &skill_md("review", "project version", "project body"),
        );

        let set = SkillSet::from_roots(&[
            (user, SkillSource::User),
            (project, SkillSource::Project),
        ]);
        assert_eq!(set.len(), 1);
        assert_eq!(set.get("review").unwrap().description, "project version");
        assert_eq!(set.get("review").unwrap().source, SkillSource::Project);
    }

    #[test]
    fn name_falls_back_to_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("my-skill/SKILL.md"),
            "# no frontmatter\ninstructions",
        );
        let set = SkillSet::from_roots(&[(dir.path().to_path_buf(), SkillSource::User)]);
        assert_eq!(set.names(), vec!["my-skill"]);
        assert_eq!(set.get("my-skill").unwrap().description, "no frontmatter");
    }

    #[test]
    fn prompt_block_lists_every_skill() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("a/SKILL.md"),
            &skill_md("a", "first skill", "body a"),
        );
        write(
            &dir.path().join("b/SKILL.md"),
            &skill_md("b", "second skill", "body b"),
        );
        let set = SkillSet::from_roots(&[(dir.path().to_path_buf(), SkillSource::User)]);
        let block = set.prompt_block().unwrap();
        assert!(block.contains("<skills>"));
        assert!(block.contains("- a: first skill"));
        assert!(block.contains("- b: second skill"));
    }

    #[test]
    fn empty_set_has_no_prompt_block() {
        assert!(SkillSet::default().prompt_block().is_none());
    }

    #[test]
    fn invalid_names_fall_back() {
        assert!(is_valid_name("rust-review"));
        assert!(is_valid_name("review_2"));
        assert!(!is_valid_name("Rust Review"));
        assert!(!is_valid_name(""));
    }
}
