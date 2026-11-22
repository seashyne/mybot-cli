//! Plugins: a directory of skills and prompt commands that extends mybot.
//!
//! Layout:
//!
//! ```text
//! ~/.mybot/plugins/<plugin>/
//!   plugin.toml          name, version, description
//!   skills/<name>/SKILL.md
//!   commands/<name>.md   prompt template, `$ARGUMENTS` is substituted
//! ```
//!
//! Project plugins live in `<cwd>/.mybot/plugins/` and win over user plugins
//! with the same name.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Plugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub root: PathBuf,
    pub project: bool,
}

#[derive(Debug, Clone)]
pub struct PluginCommand {
    pub name: String,
    pub description: String,
    pub body: String,
    pub plugin: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct PluginSet {
    pub plugins: Vec<Plugin>,
    pub commands: Vec<PluginCommand>,
}

impl PluginSet {
    pub fn discover(cwd: &Path) -> Self {
        let mut set = Self::default();

        for (root, project) in roots(cwd) {
            let Some(plugin) = load_plugin(&root, project) else {
                continue;
            };
            for command in load_commands(&plugin) {
                set.commands.retain(|existing| existing.name != command.name);
                set.commands.push(command);
            }
            set.plugins.retain(|existing| existing.name != plugin.name);
            set.plugins.push(plugin);
        }

        set
    }

    pub fn command(&self, name: &str) -> Option<&PluginCommand> {
        self.commands.iter().find(|command| command.name == name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.plugins.iter().map(|plugin| plugin.name.as_str()).collect()
    }
}

/// Plugin roots, user first so project plugins override them.
pub fn roots(cwd: &Path) -> Vec<(PathBuf, bool)> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.extend(scan(Some(home.join(".mybot/plugins")), false));
    }
    roots.extend(scan(Some(cwd.join(".mybot/plugins")), true));
    roots
}

fn scan(dir: Option<PathBuf>, project: bool) -> Vec<(PathBuf, bool)> {
    let Some(dir) = dir else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, bool)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|path| (path, project))
        .collect();
    out.sort();
    out
}

fn load_plugin(root: &Path, project: bool) -> Option<Plugin> {
    let manifest_path = root.join("plugin.toml");
    let fallback = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    let manifest = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|text| toml::from_str::<Manifest>(&text).ok())
        .unwrap_or(Manifest {
            name: fallback,
            version: String::new(),
            description: String::new(),
        });

    if manifest.name.is_empty() {
        return None;
    }

    Some(Plugin {
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        root: root.to_path_buf(),
        project,
    })
}

fn load_commands(plugin: &Plugin) -> Vec<PluginCommand> {
    let dir = plugin.root.join("commands");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut commands: Vec<PluginCommand> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).ok()?;
            let (meta, body) = super::skills::split_frontmatter(&text);
            let description = meta.get("description").cloned().unwrap_or_else(|| {
                body.lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("")
                    .trim_start_matches('#')
                    .trim()
                    .to_string()
            });
            Some(PluginCommand {
                name,
                description,
                body,
                plugin: plugin.name.clone(),
                path,
            })
        })
        .collect();

    commands.sort_by(|a, b| a.name.cmp(&b.name));
    commands
}

impl PluginCommand {
    /// Fill in the user's arguments. `$ARGUMENTS` is replaced; without a
    /// placeholder the arguments are appended as a final line.
    pub fn render(&self, arguments: &str) -> String {
        let arguments = arguments.trim();
        if self.body.contains("$ARGUMENTS") {
            return self.body.replace("$ARGUMENTS", arguments);
        }
        if arguments.is_empty() {
            return self.body.clone();
        }
        format!("{}\n\nArguments: {arguments}", self.body.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn discovers_a_project_plugin_and_its_commands() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".mybot/plugins/demo");
        write(
            &root.join("plugin.toml"),
            "name = \"demo\"\nversion = \"0.1.0\"\ndescription = \"demo plugin\"\n",
        );
        write(
            &root.join("commands/review.md"),
            "---\ndescription: Review a change\n---\nReview this diff:\n$ARGUMENTS\n",
        );

        let set = PluginSet::discover(dir.path());
        assert_eq!(set.plugins.len(), 1);
        assert_eq!(set.plugins[0].name, "demo");
        assert!(set.plugins[0].project);

        let command = set.command("review").unwrap();
        assert_eq!(command.description, "Review a change");
        assert_eq!(command.render("HEAD~1").trim(), "Review this diff:\nHEAD~1");
    }

    #[test]
    fn appends_arguments_when_there_is_no_placeholder() {
        let command = PluginCommand {
            name: "explain".into(),
            description: String::new(),
            body: "Explain the selected code.".into(),
            plugin: "demo".into(),
            path: PathBuf::new(),
        };
        assert_eq!(
            command.render("src/main.rs"),
            "Explain the selected code.\n\nArguments: src/main.rs"
        );
        assert_eq!(command.render(""), "Explain the selected code.");
    }

    #[test]
    fn missing_manifest_falls_back_to_the_directory_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".mybot/plugins/quick");
        std::fs::create_dir_all(&root).unwrap();

        let set = PluginSet::discover(dir.path());
        assert_eq!(set.names(), vec!["quick"]);
    }
}
