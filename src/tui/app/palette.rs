use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Ctx};
use crate::tui::commands::{self, CommandSpec};

#[derive(Default)]
pub struct Palette {
    pub selected: usize,
    last_query: String,
}

impl App {
    /// The palette is open while the composer holds a single-line `/query`.
    pub fn palette_query(&self) -> Option<String> {
        let text = self.composer.text();
        let rest = text.strip_prefix('/')?;
        if rest.contains('\n') || rest.chars().any(char::is_whitespace) {
            return None;
        }
        Some(rest.to_string())
    }

    pub fn palette_matches(&self) -> Vec<&'static CommandSpec> {
        match self.palette_query() {
            Some(query) => commands::matches(&query),
            None => Vec::new(),
        }
    }

    /// Keep the palette in sync with the composer after any edit.
    pub fn sync_palette(&mut self) {
        match self.palette_query() {
            Some(query) => {
                let palette = self.palette.get_or_insert_with(Palette::default);
                if palette.last_query != query {
                    palette.last_query = query;
                    palette.selected = 0;
                }
            }
            None => self.palette = None,
        }
    }

    pub(crate) fn handle_palette_key(&mut self, key: KeyEvent, ctx: &Ctx) -> bool {
        if self.palette.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.palette = None;
                self.composer.clear();
            }
            KeyCode::Up => self.move_palette(-1),
            KeyCode::Down => self.move_palette(1),
            KeyCode::PageUp => self.move_palette(-8),
            KeyCode::PageDown => self.move_palette(8),
            KeyCode::Tab => self.complete_palette(),
            KeyCode::Enter => {
                let exact = self
                    .palette_query()
                    .and_then(|query| commands::find(&query));
                if exact.is_some() {
                    self.palette = None;
                    self.submit(ctx);
                } else {
                    let matches = self.palette_matches();
                    let selected = self
                        .palette
                        .as_ref()
                        .map(|palette| palette.selected)
                        .unwrap_or(0);
                    match matches.get(selected) {
                        Some(command) if command.args.is_empty() => {
                            self.complete_with(command);
                            self.submit(ctx);
                        }
                        Some(command) => self.complete_with(command),
                        None => {
                            self.palette = None;
                            self.submit(ctx);
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    fn move_palette(&mut self, delta: isize) {
        let len = self.palette_matches().len();
        if len == 0 {
            return;
        }
        if let Some(palette) = &mut self.palette {
            let next = palette.selected as isize + delta;
            palette.selected = next.rem_euclid(len as isize) as usize;
        }
    }

    fn complete_palette(&mut self) {
        let matches = self.palette_matches();
        let selected = self
            .palette
            .as_ref()
            .map(|palette| palette.selected)
            .unwrap_or(0);
        if let Some(command) = matches.get(selected) {
            self.complete_with(command);
        }
    }

    fn complete_with(&mut self, command: &CommandSpec) {
        let text = if command.args.is_empty() {
            format!("/{}", command.name)
        } else {
            format!("/{} ", command.name)
        };
        self.composer.set_text(&text);
        self.palette = None;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn slash_text_opens_and_filters() {
        let mut app = crate::tui::app::tests::test_app();
        app.composer.insert_str("/mod");
        app.sync_palette();
        assert!(app.palette.is_some());
        assert_eq!(app.palette_matches()[0].name, "model");

        app.composer.clear();
        app.sync_palette();
        assert!(app.palette.is_none());
    }

    #[test]
    fn a_space_closes_the_palette() {
        let mut app = crate::tui::app::tests::test_app();
        app.composer.insert_str("/model gpt");
        app.sync_palette();
        assert!(app.palette.is_none());
    }

    #[test]
    fn tab_completes_the_selected_command() {
        let mut app = crate::tui::app::tests::test_app();
        app.composer.insert_str("/perm");
        app.sync_palette();
        app.complete_palette();
        assert_eq!(app.composer.text(), "/permissions ");
        assert!(app.palette.is_none());
    }

    #[test]
    fn filtering_resets_the_selection() {
        let mut app = crate::tui::app::tests::test_app();
        app.composer.insert_str("/");
        app.sync_palette();
        app.move_palette(3);
        assert_eq!(app.palette.as_ref().unwrap().selected, 3);

        app.composer.insert_str("mo");
        app.sync_palette();
        assert_eq!(app.palette.as_ref().unwrap().selected, 0);
        assert_eq!(app.palette_matches()[0].name, "model");
    }
}
