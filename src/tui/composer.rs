/// Minimal multiline input buffer with a character-indexed cursor and history.
#[derive(Debug)]
pub struct Composer {
    lines: Vec<String>,
    row: usize,
    col: usize,
    history: Vec<String>,
    history_pos: Option<usize>,
    draft: Option<Vec<String>>,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            history: Vec::new(),
            history_pos: None,
            draft: None,
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.row = 0;
        self.col = 0;
        self.history_pos = None;
        self.draft = None;
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = text.split('\n').map(str::to_string).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].chars().count();
    }

    pub fn insert_char(&mut self, value: char) {
        let byte = byte_index(&self.lines[self.row], self.col);
        self.lines[self.row].insert(byte, value);
        self.col += 1;
    }

    pub fn insert_str(&mut self, value: &str) {
        for ch in value.chars() {
            if ch == '\n' {
                self.newline();
            } else {
                self.insert_char(ch);
            }
        }
    }

    pub fn newline(&mut self) {
        let byte = byte_index(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(byte);
        self.row += 1;
        self.lines.insert(self.row, tail);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            let end = byte_index(&self.lines[self.row], self.col);
            let start = byte_index(&self.lines[self.row], self.col - 1);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
            self.lines[self.row].push_str(&current);
        }
    }

    pub fn delete(&mut self) {
        let length = self.lines[self.row].chars().count();
        if self.col < length {
            let start = byte_index(&self.lines[self.row], self.col);
            let end = byte_index(&self.lines[self.row], self.col + 1);
            self.lines[self.row].replace_range(start..end, "");
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
        }
    }

    pub fn right(&mut self) {
        if self.col < self.lines[self.row].chars().count() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.lines[self.row].chars().count());
        }
    }

    pub fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.lines[self.row].chars().count());
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.lines[self.row].chars().count();
    }

    /// Take the buffer contents, record them in history, and reset.
    pub fn take(&mut self) -> String {
        let text = self.text();
        if !text.trim().is_empty()
            && self.history.last().map(|last| last != &text).unwrap_or(true)
        {
            self.history.push(text.clone());
        }
        self.clear();
        text
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_pos {
            None => {
                self.draft = Some(self.lines.clone());
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(position) => position - 1,
        };
        self.history_pos = Some(next);
        let text = self.history[next].clone();
        self.set_text(&text);
    }

    pub fn history_next(&mut self) {
        match self.history_pos {
            None => {}
            Some(position) if position + 1 < self.history.len() => {
                self.history_pos = Some(position + 1);
                let text = self.history[position + 1].clone();
                self.set_text(&text);
            }
            Some(_) => {
                self.history_pos = None;
                self.lines = self
                    .draft
                    .take()
                    .unwrap_or_else(|| vec![String::new()]);
                self.row = self.lines.len() - 1;
                self.col = self.lines[self.row].chars().count();
            }
        }
    }

    /// Return `(at_char_col, query)` when the cursor is inside a whitespace-delimited `@query`.
    pub fn mention_at_cursor(&self) -> Option<(usize, String)> {
        let line = self.lines.get(self.row)?;
        let chars: Vec<char> = line.chars().collect();
        let col = self.col.min(chars.len());
        let mut start = col;
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        if start < col && chars[start] == '@' {
            let query: String = chars[start + 1..col].iter().collect();
            return Some((start, query));
        }
        None
    }

    /// Replace the `@query` token starting at `start_col` up to the cursor with `@insertion`.
    pub fn complete_mention(&mut self, start_col: usize, insertion: &str) {
        let Some(line) = self.lines.get_mut(self.row) else {
            return;
        };
        let total = line.chars().count();
        let start = start_col.min(total);
        let end = self.col.min(total);
        if start > end {
            return;
        }
        let start_byte = byte_index(line, start);
        let end_byte = byte_index(line, end);
        let needs_space = line[end_byte..]
            .chars()
            .next()
            .map(|ch| !ch.is_whitespace())
            .unwrap_or(true);
        let replacement = if needs_space {
            format!("@{insertion} ")
        } else {
            format!("@{insertion}")
        };
        line.replace_range(start_byte..end_byte, &replacement);
        self.col = start + replacement.chars().count();
    }
}

fn byte_index(line: &str, char_index: usize) -> usize {
    line.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_and_deletes() {
        let mut composer = Composer::new();
        composer.insert_str("hello");
        assert_eq!(composer.text(), "hello");
        composer.backspace();
        assert_eq!(composer.text(), "hell");
        composer.home();
        composer.delete();
        assert_eq!(composer.text(), "ell");
    }

    #[test]
    fn multiline_editing() {
        let mut composer = Composer::new();
        composer.insert_str("one");
        composer.newline();
        composer.insert_str("two");
        assert_eq!(composer.text(), "one\ntwo");
        assert_eq!(composer.cursor(), (1, 3));
        composer.up();
        composer.end();
        composer.insert_str("!");
        assert_eq!(composer.text(), "one!\ntwo");
    }

    #[test]
    fn backspace_joins_lines() {
        let mut composer = Composer::new();
        composer.insert_str("a");
        composer.newline();
        composer.insert_str("b");
        composer.backspace();
        composer.backspace();
        assert_eq!(composer.text(), "a");
    }

    #[test]
    fn history_recalls_previous_input() {
        let mut composer = Composer::new();
        composer.insert_str("first");
        composer.take();
        composer.insert_str("second");
        composer.take();

        composer.history_prev();
        assert_eq!(composer.text(), "second");
        composer.history_prev();
        assert_eq!(composer.text(), "first");
        composer.history_next();
        assert_eq!(composer.text(), "second");
        composer.history_next();
        assert!(composer.is_empty());
    }

    #[test]
    fn unicode_cursor_is_safe() {
        let mut composer = Composer::new();
        composer.insert_str("héllo");
        composer.left();
        composer.insert_char('!');
        assert_eq!(composer.text(), "héll!o");
    }

    #[test]
    fn detects_and_completes_mentions() {
        let mut composer = Composer::new();
        composer.insert_str("check @src/ma");
        assert_eq!(composer.mention_at_cursor(), Some((6, "src/ma".into())));
        composer.complete_mention(6, "src/main.rs");
        assert_eq!(composer.text(), "check @src/main.rs ");
        assert_eq!(composer.mention_at_cursor(), None);

        composer.clear();
        composer.insert_str("foo@bar");
        assert_eq!(composer.mention_at_cursor(), None);
    }
}
