use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Ctx};

const MAX_FILES: usize = 2000;
const MAX_MATCHES: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    File,
    Plugin,
    Skill,
}

impl MentionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Plugin => "plugin",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MentionItem {
    pub kind: MentionKind,
    pub label: String,
    pub insert: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct Mention {
    pub items: Vec<MentionItem>,
    pub selected: usize,
    pub start_col: usize,
    pub query: String,
}

impl Mention {
    pub fn discover(cwd: &Path, ctx: &Ctx, start_col: usize, query: String) -> Self {
        let mut items = Vec::new();
        for plugin in &ctx.plugins.plugins {
            items.push(MentionItem {
                kind: MentionKind::Plugin,
                label: plugin.name.clone(),
                insert: format!("plugin:{}", plugin.name),
                detail: plugin.description.clone(),
            });
        }
        for skill in &ctx.skills.skills {
            items.push(MentionItem {
                kind: MentionKind::Skill,
                label: skill.name.clone(),
                insert: format!("skill:{}", skill.name),
                detail: skill.description.clone(),
            });
        }
        for path in scan_workspace_files(cwd) {
            items.push(MentionItem {
                kind: MentionKind::File,
                label: path.clone(),
                insert: path,
                detail: String::new(),
            });
        }
        Self {
            items,
            selected: 0,
            start_col,
            query,
        }
    }

    #[cfg(test)]
    pub fn from_items(items: Vec<MentionItem>, start_col: usize, query: String) -> Self {
        Self {
            items,
            selected: 0,
            start_col,
            query,
        }
    }

    pub fn matches(&self) -> Vec<&MentionItem> {
        let query = self.query.trim().to_ascii_lowercase();
        let mut ranked: Vec<(i32, usize, &MentionItem)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| score(&query, item).map(|s| (s, index, item)))
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        ranked
            .into_iter()
            .take(MAX_MATCHES)
            .map(|(_, _, item)| item)
            .collect()
    }
}

fn scan_workspace_files(cwd: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(cwd)
        .hidden(false)
        .git_ignore(true)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();

    for entry in walker.flatten() {
        if files.len() >= MAX_FILES {
            break;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let relative = entry.path().strip_prefix(cwd).unwrap_or(entry.path());
        let text = relative.to_string_lossy().replace('\\', "/");
        if !text.is_empty() {
            files.push(text);
        }
    }
    files.sort();
    files
}

fn score(query: &str, item: &MentionItem) -> Option<i32> {
    if query.is_empty() {
        let boost = match item.kind {
            MentionKind::Plugin => 20,
            MentionKind::Skill => 10,
            MentionKind::File => 0,
        };
        return Some(boost);
    }

    let label = item.label.to_ascii_lowercase();
    let basename = label.rsplit('/').next().unwrap_or(&label);

    if label == query || basename == query {
        return Some(1000);
    }
    if basename.starts_with(query) {
        return Some(900 - label.len() as i32);
    }
    if label.starts_with(query) {
        return Some(850 - label.len() as i32);
    }
    if basename.contains(query) {
        return Some(750 - label.len() as i32);
    }
    if label.contains(query) {
        return Some(650 - label.len() as i32);
    }
    if is_subsequence(query, &label) {
        return Some(450 - label.len() as i32);
    }
    None
}

fn is_subsequence(query: &str, text: &str) -> bool {
    let mut chars = text.chars();
    query
        .chars()
        .all(|wanted| chars.any(|candidate| candidate == wanted))
}

impl App {
    pub fn mention_matches(&self) -> Vec<&MentionItem> {
        match &self.mention {
            Some(mention) => mention.matches(),
            None => Vec::new(),
        }
    }

    /// Keep the `@` mention popup synchronized with the composer cursor.
    pub fn sync_mention(&mut self, ctx: &Ctx) {
        if self.palette.is_some() {
            self.mention = None;
            self.mention_dismissed = None;
            return;
        }
        let Some((start_col, query)) = self.composer.mention_at_cursor() else {
            self.mention = None;
            self.mention_dismissed = None;
            return;
        };
        let key = (self.composer.cursor().0, start_col);
        if self.mention_dismissed == Some(key) {
            return;
        }
        match &mut self.mention {
            Some(mention) if mention.start_col == start_col => {
                if mention.query != query {
                    mention.query = query;
                    mention.selected = 0;
                }
            }
            _ => {
                self.mention = Some(Mention::discover(&self.cwd, ctx, start_col, query));
            }
        }
    }

    pub(crate) fn handle_mention_key(&mut self, key: KeyEvent) -> bool {
        if self.mention.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                if let Some(mention) = self.mention.take() {
                    self.mention_dismissed = Some((self.composer.cursor().0, mention.start_col));
                }
            }
            KeyCode::Up => self.move_mention(-1),
            KeyCode::Down => self.move_mention(1),
            KeyCode::PageUp => self.move_mention(-8),
            KeyCode::PageDown => self.move_mention(8),
            KeyCode::Tab | KeyCode::Enter => {
                if !self.complete_mention() && key.code == KeyCode::Enter {
                    self.mention = None;
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    fn move_mention(&mut self, delta: isize) {
        let len = self.mention_matches().len();
        if len == 0 {
            return;
        }
        if let Some(mention) = &mut self.mention {
            let next = mention.selected as isize + delta;
            mention.selected = next.rem_euclid(len as isize) as usize;
        }
    }

    fn complete_mention(&mut self) -> bool {
        let Some(mention) = &self.mention else {
            return false;
        };
        let matches = mention.matches();
        let Some(item) = matches.get(mention.selected) else {
            return false;
        };
        let start_col = mention.start_col;
        let insert = item.insert.clone();
        self.composer.complete_mention(start_col, &insert);
        self.mention = None;
        self.mention_dismissed = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_items() -> Vec<MentionItem> {
        vec![
            MentionItem {
                kind: MentionKind::Plugin,
                label: "git-helper".into(),
                insert: "plugin:git-helper".into(),
                detail: "Git shortcuts".into(),
            },
            MentionItem {
                kind: MentionKind::File,
                label: "src/main.rs".into(),
                insert: "src/main.rs".into(),
                detail: String::new(),
            },
            MentionItem {
                kind: MentionKind::File,
                label: "src/tui/render.rs".into(),
                insert: "src/tui/render.rs".into(),
                detail: String::new(),
            },
        ]
    }

    #[test]
    fn ranks_basename_and_exact_matches_first() {
        let mention = Mention::from_items(sample_items(), 0, "main".into());
        let matches = mention.matches();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label, "src/main.rs");
    }

    #[test]
    fn completing_mention_replaces_token_and_closes_popup() {
        let mut app = crate::tui::app::tests::test_app();
        app.composer.insert_str("open @ren");
        app.mention = Some(Mention::from_items(sample_items(), 5, "ren".into()));

        assert!(app.handle_mention_key(KeyEvent::from(KeyCode::Tab)));
        assert_eq!(app.composer.text(), "open @src/tui/render.rs ");
        assert!(app.mention.is_none());
    }

    #[test]
    fn escape_dismisses_mention_without_clearing_composer() {
        let mut app = crate::tui::app::tests::test_app();
        app.composer.insert_str("open @src");
        app.mention = Some(Mention::from_items(sample_items(), 5, "src".into()));

        assert!(app.handle_mention_key(KeyEvent::from(KeyCode::Esc)));
        assert_eq!(app.composer.text(), "open @src");
        assert!(app.mention.is_none());
        assert_eq!(app.mention_dismissed, Some((0, 5)));
    }
}
