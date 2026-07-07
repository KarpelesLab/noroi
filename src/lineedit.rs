//! A reusable single-line text editor — the "line editor" of a shell prompt.
//!
//! [`LineEditor`] owns an editable string and a cursor, and turns
//! [`KeyEvent`]s into edits: insertion, character/word/line deletion, cursor
//! motion (including word hops and line ends), and an optional input history
//! browsable with Up/Down. It is `#![no_std]`-friendly and knows nothing about
//! the terminal — [`LineEditor::render`] paints into a [`Buffer`] and returns
//! the on-screen cursor position so the caller can place the hardware cursor.
//!
//! ```
//! use noroi::event::{KeyCode, KeyEvent};
//! use noroi::lineedit::{LineEditor, LineOutcome};
//!
//! let mut ed = LineEditor::new();
//! for c in "hi".chars() {
//!     ed.handle_key(KeyEvent::new(KeyCode::Char(c)));
//! }
//! assert_eq!(ed.text(), "hi");
//! assert_eq!(ed.handle_key(KeyEvent::new(KeyCode::Enter)), LineOutcome::Submitted);
//! ```

use crate::buffer::{char_width, Buffer};
use crate::event::{KeyCode, KeyEvent, Modifiers};
use crate::geom::{Point, Rect};
use crate::style::Style;
use alloc::string::String;
use alloc::vec::Vec;

/// What a [`LineEditor::handle_key`] call did with the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOutcome {
    /// The buffer or cursor changed.
    Changed,
    /// Enter was pressed; the line is submitted (and pushed to history).
    Submitted,
    /// Escape was pressed.
    Cancelled,
    /// The key was not one this editor handles.
    Ignored,
}

/// An editable line of text with a cursor and optional history.
#[derive(Debug, Clone, Default)]
pub struct LineEditor {
    text: String,
    /// Cursor as a byte offset into `text` (always on a char boundary).
    cursor: usize,
    /// Leftmost visible display column, updated during [`render`](Self::render).
    scroll: u16,
    history: Vec<String>,
    /// `None` = editing the live line; `Some(i)` = viewing `history[i]`.
    history_pos: Option<usize>,
    /// The live line stashed while browsing history.
    stash: String,
    max_history: usize,
}

impl LineEditor {
    /// A new, empty editor with a default history capacity.
    pub fn new() -> Self {
        LineEditor { max_history: 256, ..Default::default() }
    }

    /// The current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replace the text and move the cursor to the end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.history_pos = None;
    }

    /// Clear the line and reset the cursor.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.scroll = 0;
        self.history_pos = None;
    }

    /// The cursor's byte offset within [`text`](Self::text).
    pub fn cursor_byte(&self) -> usize {
        self.cursor
    }

    /// The cursor's display column (sum of preceding character widths).
    pub fn cursor_column(&self) -> u16 {
        self.text[..self.cursor].chars().map(char_width).sum()
    }

    /// Push a submitted line into the history ring.
    fn record_history(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if self.history.last().map(String::as_str) == Some(line) {
            return;
        }
        self.history.push(String::from(line));
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Handle a key press, mutating the line. See [`LineOutcome`].
    pub fn handle_key(&mut self, key: KeyEvent) -> LineOutcome {
        let ctrl = key.modifiers.contains(Modifiers::CTRL);
        let alt = key.modifiers.contains(Modifiers::ALT);
        match key.code {
            KeyCode::Char(c) if ctrl => self.handle_ctrl_char(c),
            KeyCode::Char(c) if alt => self.handle_alt_char(c),
            KeyCode::Char(c) => {
                self.insert_char(c);
                LineOutcome::Changed
            }
            KeyCode::Enter => {
                let line = core::mem::take(&mut self.text);
                self.record_history(&line);
                self.text = line;
                LineOutcome::Submitted
            }
            KeyCode::Esc => LineOutcome::Cancelled,
            KeyCode::Backspace => self.delete_backward(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left if ctrl => self.move_word_left(),
            KeyCode::Right if ctrl => self.move_word_right(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.move_home(),
            KeyCode::End => self.move_end(),
            KeyCode::Up => self.history_prev(),
            KeyCode::Down => self.history_next(),
            _ => LineOutcome::Ignored,
        }
    }

    fn handle_ctrl_char(&mut self, c: char) -> LineOutcome {
        match c {
            'a' => self.move_home(),
            'e' => self.move_end(),
            'b' => self.move_left(),
            'f' => self.move_right(),
            'h' => self.delete_backward(),
            'd' => self.delete_forward(),
            'w' => self.delete_word_backward(),
            'k' => self.kill_to_end(),
            'u' => self.kill_to_start(),
            _ => LineOutcome::Ignored,
        }
    }

    fn handle_alt_char(&mut self, c: char) -> LineOutcome {
        match c {
            'b' => self.move_word_left(),
            'f' => self.move_word_right(),
            'd' => self.delete_word_forward(),
            _ => LineOutcome::Ignored,
        }
    }

    // ----- editing -------------------------------------------------------

    fn insert_char(&mut self, c: char) {
        self.detach_history();
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert a whole string (e.g. from a paste) at the cursor.
    pub fn insert_str(&mut self, s: &str) {
        self.detach_history();
        // Strip embedded newlines; a single-line editor cannot hold them.
        for part in s.split(['\n', '\r']) {
            self.text.insert_str(self.cursor, part);
            self.cursor += part.len();
        }
    }

    fn delete_backward(&mut self) -> LineOutcome {
        self.detach_history();
        if self.cursor == 0 {
            return LineOutcome::Ignored;
        }
        let start = self.prev_boundary(self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        LineOutcome::Changed
    }

    fn delete_forward(&mut self) -> LineOutcome {
        self.detach_history();
        if self.cursor >= self.text.len() {
            return LineOutcome::Ignored;
        }
        let end = self.next_boundary(self.cursor);
        self.text.replace_range(self.cursor..end, "");
        LineOutcome::Changed
    }

    fn delete_word_backward(&mut self) -> LineOutcome {
        self.detach_history();
        let start = self.word_start(self.cursor);
        if start == self.cursor {
            return LineOutcome::Ignored;
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        LineOutcome::Changed
    }

    fn delete_word_forward(&mut self) -> LineOutcome {
        self.detach_history();
        let end = self.word_end(self.cursor);
        if end == self.cursor {
            return LineOutcome::Ignored;
        }
        self.text.replace_range(self.cursor..end, "");
        LineOutcome::Changed
    }

    fn kill_to_end(&mut self) -> LineOutcome {
        self.detach_history();
        if self.cursor >= self.text.len() {
            return LineOutcome::Ignored;
        }
        self.text.truncate(self.cursor);
        LineOutcome::Changed
    }

    fn kill_to_start(&mut self) -> LineOutcome {
        self.detach_history();
        if self.cursor == 0 {
            return LineOutcome::Ignored;
        }
        self.text.replace_range(0..self.cursor, "");
        self.cursor = 0;
        LineOutcome::Changed
    }

    // ----- motion --------------------------------------------------------

    fn move_left(&mut self) -> LineOutcome {
        if self.cursor == 0 {
            return LineOutcome::Ignored;
        }
        self.cursor = self.prev_boundary(self.cursor);
        LineOutcome::Changed
    }

    fn move_right(&mut self) -> LineOutcome {
        if self.cursor >= self.text.len() {
            return LineOutcome::Ignored;
        }
        self.cursor = self.next_boundary(self.cursor);
        LineOutcome::Changed
    }

    fn move_home(&mut self) -> LineOutcome {
        self.cursor = 0;
        LineOutcome::Changed
    }

    fn move_end(&mut self) -> LineOutcome {
        self.cursor = self.text.len();
        LineOutcome::Changed
    }

    fn move_word_left(&mut self) -> LineOutcome {
        let start = self.word_start(self.cursor);
        if start == self.cursor {
            return LineOutcome::Ignored;
        }
        self.cursor = start;
        LineOutcome::Changed
    }

    fn move_word_right(&mut self) -> LineOutcome {
        let end = self.word_end(self.cursor);
        if end == self.cursor {
            return LineOutcome::Ignored;
        }
        self.cursor = end;
        LineOutcome::Changed
    }

    // ----- history -------------------------------------------------------

    fn detach_history(&mut self) {
        self.history_pos = None;
    }

    fn history_prev(&mut self) -> LineOutcome {
        if self.history.is_empty() {
            return LineOutcome::Ignored;
        }
        let new_pos = match self.history_pos {
            None => {
                self.stash = self.text.clone();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_pos = Some(new_pos);
        self.text = self.history[new_pos].clone();
        self.cursor = self.text.len();
        LineOutcome::Changed
    }

    fn history_next(&mut self) -> LineOutcome {
        match self.history_pos {
            None => LineOutcome::Ignored,
            Some(i) if i + 1 < self.history.len() => {
                self.history_pos = Some(i + 1);
                self.text = self.history[i + 1].clone();
                self.cursor = self.text.len();
                LineOutcome::Changed
            }
            Some(_) => {
                // Past the newest entry: restore the stashed live line.
                self.history_pos = None;
                self.text = core::mem::take(&mut self.stash);
                self.cursor = self.text.len();
                LineOutcome::Changed
            }
        }
    }

    // ----- boundary helpers ---------------------------------------------

    fn prev_boundary(&self, byte: usize) -> usize {
        let mut i = byte.saturating_sub(1);
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn next_boundary(&self, byte: usize) -> usize {
        let mut i = (byte + 1).min(self.text.len());
        while i < self.text.len() && !self.text.is_char_boundary(i) {
            i += 1;
        }
        i
    }

    /// Byte offset of the start of the word at/left of `byte` (skips spaces,
    /// then non-spaces).
    fn word_start(&self, byte: usize) -> usize {
        let bytes = self.text.as_bytes();
        let mut i = byte;
        while i > 0 && bytes[i - 1] == b' ' {
            i = self.prev_boundary(i);
        }
        while i > 0 && bytes[i - 1] != b' ' {
            i = self.prev_boundary(i);
        }
        i
    }

    /// Byte offset of the end of the word at/right of `byte`.
    fn word_end(&self, byte: usize) -> usize {
        let bytes = self.text.as_bytes();
        let len = self.text.len();
        let mut i = byte;
        while i < len && bytes[i] == b' ' {
            i = self.next_boundary(i);
        }
        while i < len && bytes[i] != b' ' {
            i = self.next_boundary(i);
        }
        i
    }

    // ----- rendering -----------------------------------------------------

    /// Render the line into a single-row `area` of `buf` with `style`, scrolling
    /// horizontally to keep the cursor visible. Returns the on-screen cursor
    /// [`Point`] so the caller can position the terminal cursor.
    pub fn render(&mut self, area: Rect, buf: &mut Buffer, style: Style) -> Point {
        if area.is_empty() {
            return area.top_left();
        }
        let width = area.width;
        let cursor_col = self.cursor_column();

        // Keep the cursor within [scroll, scroll + width - 1].
        if cursor_col < self.scroll {
            self.scroll = cursor_col;
        } else if cursor_col >= self.scroll + width {
            self.scroll = cursor_col - width + 1;
        }

        // Paint from the first character at or after `scroll` display columns.
        let mut col = 0u16; // display column within the full text
        let mut x = area.x;
        for c in self.text.chars() {
            let cw = char_width(c).max(1);
            if col + cw <= self.scroll {
                col += cw;
                continue;
            }
            if x >= area.right() {
                break;
            }
            let adv = buf.set_char(x, area.y, c, style);
            x = x.saturating_add(adv.max(1));
            col += cw;
        }
        // Blank the remainder of the field.
        while x < area.right() {
            buf.set_char(x, area.y, ' ', style);
            x += 1;
        }

        let cursor_x = area.x + cursor_col.saturating_sub(self.scroll).min(width.saturating_sub(1));
        Point::new(cursor_x, area.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c))
    }

    #[test]
    fn typing_and_backspace() {
        let mut ed = LineEditor::new();
        for c in "abc".chars() {
            ed.handle_key(key(c));
        }
        assert_eq!(ed.text(), "abc");
        ed.handle_key(KeyEvent::new(KeyCode::Backspace));
        assert_eq!(ed.text(), "ab");
    }

    #[test]
    fn cursor_motion_and_insert() {
        let mut ed = LineEditor::new();
        ed.set_text("ac");
        ed.handle_key(KeyEvent::new(KeyCode::Home));
        ed.handle_key(KeyEvent::new(KeyCode::Right));
        ed.handle_key(key('b'));
        assert_eq!(ed.text(), "abc");
    }

    #[test]
    fn word_delete() {
        let mut ed = LineEditor::new();
        ed.set_text("hello world");
        ed.handle_key(KeyEvent::with(KeyCode::Char('w'), Modifiers::CTRL));
        assert_eq!(ed.text(), "hello ");
    }

    #[test]
    fn word_left_right() {
        let mut ed = LineEditor::new();
        ed.set_text("foo bar");
        ed.handle_key(KeyEvent::with(KeyCode::Left, Modifiers::CTRL));
        assert_eq!(ed.cursor_byte(), 4); // start of "bar"
        ed.handle_key(KeyEvent::with(KeyCode::Left, Modifiers::CTRL));
        assert_eq!(ed.cursor_byte(), 0);
    }

    #[test]
    fn history_browse() {
        let mut ed = LineEditor::new();
        ed.set_text("first");
        ed.handle_key(KeyEvent::new(KeyCode::Enter));
        ed.clear();
        ed.set_text("second");
        ed.handle_key(KeyEvent::new(KeyCode::Enter));
        ed.clear();
        ed.handle_key(KeyEvent::new(KeyCode::Up));
        assert_eq!(ed.text(), "second");
        ed.handle_key(KeyEvent::new(KeyCode::Up));
        assert_eq!(ed.text(), "first");
        ed.handle_key(KeyEvent::new(KeyCode::Down));
        assert_eq!(ed.text(), "second");
    }

    #[test]
    fn kill_to_end() {
        let mut ed = LineEditor::new();
        ed.set_text("hello world");
        ed.handle_key(KeyEvent::new(KeyCode::Home));
        ed.handle_key(KeyEvent::with(KeyCode::Char('k'), Modifiers::CTRL));
        assert_eq!(ed.text(), "");
    }
}
