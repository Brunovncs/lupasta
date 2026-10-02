//! A one-line text field's state: text and a caret, edited by keystrokes. The search prompt and
//! the "skip folders" setting use it; drawing is the shell's job.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineEdit {
    pub text: String,
    /// Caret position in chars.
    pub caret: usize,
}

impl LineEdit {
    pub fn new(text: &str) -> LineEdit {
        LineEdit { text: text.to_string(), caret: text.chars().count() }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.text.char_indices().nth(char_idx).map_or(self.text.len(), |(i, _)| i)
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        let at = self.byte_at(self.caret);
        self.text.insert_str(at, &clean);
        self.caret += clean.chars().count();
    }

    pub fn backspace(&mut self, word: bool) {
        let from = if word { self.word_start() } else { self.caret.saturating_sub(1) };
        let (a, b) = (self.byte_at(from), self.byte_at(self.caret));
        self.text.replace_range(a..b, "");
        self.caret = from;
    }

    pub fn delete(&mut self) {
        if self.caret < self.len() {
            let (a, b) = (self.byte_at(self.caret), self.byte_at(self.caret + 1));
            self.text.replace_range(a..b, "");
        }
    }

    fn word_start(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.caret;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    pub fn left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.caret = (self.caret + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.caret = 0;
    }

    pub fn end(&mut self) {
        self.caret = self.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    /// Applies a key by name (GPUI's `Keystroke::key`). Returns true when the key was an editing
    /// key; typed characters go through `insert` instead.
    pub fn key(&mut self, key: &str, ctrl: bool) -> bool {
        match key {
            "backspace" => self.backspace(ctrl),
            "delete" => self.delete(),
            "left" => self.left(),
            "right" => self.right(),
            "home" => self.home(),
            "end" => self.end(),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_at_the_caret() {
        let mut e = LineEdit::new("mordor");
        e.insert(" roster");
        assert_eq!(e.text, "mordor roster");
        e.backspace(true);
        assert_eq!(e.text, "mordor ");
        e.home();
        e.insert("é");
        e.right();
        e.backspace(false);
        assert_eq!(e.text, "éordor ");
        e.end();
        e.left();
        e.delete();
        assert_eq!(e.text, "éordor");
        e.insert("\n");
        assert_eq!(e.text, "éordor");
    }
}
