use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::permissions::Decision;

use super::{App, Ctx};

impl App {
    pub(crate) fn handle_paste(&mut self, text: &str, ctx: &Ctx) {
        if let Some(picker) = &mut self.model_picker {
            picker.query.push_str(text.trim());
            picker.selected = 0;
            picker.error = None;
            return;
        }
        if self.permission.is_none() {
            self.composer.insert_str(text);
            self.sync_palette();
            self.sync_mention(ctx);
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx) {
        if self.handle_model_key(key, ctx) {
            return;
        }
        if self.handle_permission_key(key) {
            return;
        }
        if self.handle_palette_key(key, ctx) {
            return;
        }
        if self.handle_mention_key(key) {
            return;
        }

        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            KeyCode::Char('c') if control => {
                if self.busy {
                    self.cancel.cancel();
                    self.info("cancelling…");
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('d') if control => {
                if self.composer.is_empty() {
                    self.quit = true;
                }
            }
            KeyCode::Esc => {
                if self.busy {
                    self.cancel.cancel();
                    self.info("cancelling…");
                } else {
                    self.composer.clear();
                }
            }
            KeyCode::Enter if shift || alt => self.composer.newline(),
            KeyCode::Char('j') if control => self.composer.newline(),
            KeyCode::Enter => self.submit(ctx),
            KeyCode::Backspace => self.composer.backspace(),
            KeyCode::Delete => self.composer.delete(),
            KeyCode::Left => self.composer.left(),
            KeyCode::Right => self.composer.right(),
            KeyCode::Up => {
                if self.composer.is_empty() {
                    self.composer.history_prev();
                } else {
                    self.composer.up();
                }
            }
            KeyCode::Down => {
                if self.composer.is_empty() {
                    self.composer.history_next();
                } else {
                    self.composer.down();
                }
            }
            KeyCode::Home => self.composer.home(),
            KeyCode::End => self.composer.end(),
            KeyCode::Tab => self.composer.insert_str("  "),
            KeyCode::PageUp => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(5);
                self.auto_scroll = false;
            }
            KeyCode::PageDown => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(5);
                if self.scroll_from_bottom == 0 {
                    self.auto_scroll = true;
                }
            }
            KeyCode::Char(value) if !control && !alt => self.composer.insert_char(value),
            _ => {}
        }
        self.sync_palette();
        self.sync_mention(ctx);
    }

    pub(crate) fn handle_permission_key(&mut self, key: KeyEvent) -> bool {
        if self.permission.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Char('a') => self.answer_permission(Decision::AllowOnce),
            KeyCode::Char('A') => self.answer_permission(Decision::AllowAlways),
            KeyCode::Char('d') | KeyCode::Esc => self.answer_permission(Decision::Deny),
            _ => {}
        }
        true
    }

    fn answer_permission(&mut self, decision: Decision) {
        if let Some(modal) = self.permission.take() {
            modal.answer(decision);
        }
    }
}
