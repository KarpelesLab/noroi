//! The cell grid and diff engine.
//!
//! A [`Buffer`] is a rectangular grid of [`Cell`]s. Widgets never talk to the
//! terminal directly — they paint into a `Buffer`. To display a frame the
//! backend keeps two buffers (a *front* buffer holding what is on screen and a
//! *back* buffer holding the freshly painted frame) and calls [`Buffer::diff`]
//! to compute the minimal list of cells that actually changed. This is what
//! keeps redraws cheap and flicker-free, exactly as curses does.

use crate::geom::{Point, Rect};
use crate::style::Style;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// A compact holder for a cell's grapheme.
///
/// The common case — a single scalar value — is stored inline with no heap
/// allocation. Longer grapheme clusters (a base character plus combining marks)
/// spill to the heap. Either way [`Grapheme::as_str`] hands back a real `&str`.
#[derive(Clone, PartialEq, Eq)]
enum Grapheme {
    /// Valid UTF-8 stored inline; `len` bytes of `buf` are used.
    Inline { buf: [u8; 14], len: u8 },
    /// A cluster too large to inline.
    Heap(Box<str>),
}

impl Grapheme {
    fn from_char(c: char) -> Self {
        let mut buf = [0u8; 14];
        let s = c.encode_utf8(&mut buf);
        let len = s.len() as u8;
        Grapheme::Inline { buf, len }
    }

    fn from_str(s: &str) -> Self {
        let bytes = s.as_bytes();
        if bytes.len() <= 14 {
            let mut buf = [0u8; 14];
            buf[..bytes.len()].copy_from_slice(bytes);
            Grapheme::Inline { buf, len: bytes.len() as u8 }
        } else {
            Grapheme::Heap(Box::from(s))
        }
    }

    fn as_str(&self) -> &str {
        match self {
            // We only ever store valid UTF-8, so the unwrap cannot fail; keeping
            // it avoids `unsafe`, which the no_std build forbids.
            Grapheme::Inline { buf, len } => {
                core::str::from_utf8(&buf[..*len as usize]).unwrap_or(" ")
            }
            Grapheme::Heap(s) => s,
        }
    }
}

/// The displayed content of one terminal cell.
///
/// Most cells hold a single character. To support double-width characters
/// (CJK, many emoji) a wide glyph occupies its own cell and marks the following
/// cell as a [`Cell::is_continuation`] placeholder that renders nothing.
#[derive(Clone, PartialEq, Eq)]
pub struct Cell {
    symbol: Grapheme,
    /// The style applied to this cell.
    pub style: Style,
    continuation: bool,
}

impl Cell {
    /// A blank cell: a space with the default style.
    pub fn blank() -> Cell {
        Cell {
            symbol: Grapheme::Inline { buf: *b"           \0\0\0", len: 1 },
            style: Style::RESET,
            continuation: false,
        }
    }

    /// Create a cell holding a single character with default style.
    pub fn from_char(c: char) -> Self {
        Cell { symbol: Grapheme::from_char(c), style: Style::RESET, continuation: false }
    }

    /// The primary character of the cell (first scalar of a cluster).
    pub fn symbol_char(&self) -> char {
        self.symbol.as_str().chars().next().unwrap_or(' ')
    }

    /// The full grapheme as a string slice.
    pub fn symbol(&self) -> &str {
        self.symbol.as_str()
    }

    /// Set the cell to a single character, preserving the style.
    pub fn set_char(&mut self, c: char) -> &mut Self {
        self.symbol = Grapheme::from_char(c);
        self.continuation = false;
        self
    }

    /// Set the cell to a grapheme cluster (multiple scalars), preserving style.
    pub fn set_symbol(&mut self, s: &str) -> &mut Self {
        self.symbol = if s.is_empty() { Grapheme::from_char(' ') } else { Grapheme::from_str(s) };
        self.continuation = false;
        self
    }

    /// Replace the style.
    pub fn set_style(&mut self, style: Style) -> &mut Self {
        self.style = style;
        self
    }

    /// Overlay `style` on top of the current one (see [`Style::patch`]).
    pub fn patch_style(&mut self, style: Style) -> &mut Self {
        self.style = self.style.patch(style);
        self
    }

    /// True if this is the hidden right half of a wide glyph.
    pub fn is_continuation(&self) -> bool {
        self.continuation
    }

    /// Reset to a blank cell.
    pub fn reset(&mut self) {
        self.symbol = Grapheme::from_char(' ');
        self.style = Style::RESET;
        self.continuation = false;
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell::blank()
    }
}

impl core::fmt::Debug for Cell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cell")
            .field("symbol", &self.symbol.as_str())
            .field("style", &self.style)
            .field("continuation", &self.continuation)
            .finish()
    }
}

/// A rectangular grid of [`Cell`]s positioned somewhere on the screen.
///
/// The buffer stores its own [`area`](Buffer::area); cell `(x, y)` is addressed
/// in absolute screen coordinates, and indexing outside the area is ignored on
/// writes and yields `None` on reads. This makes it safe to clip widgets that
/// paint slightly out of bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffer {
    area: Rect,
    cells: Vec<Cell>,
}

impl Buffer {
    /// Create a buffer covering `area`, filled with blank cells.
    pub fn empty(area: Rect) -> Self {
        Buffer { area, cells: vec![Cell::blank(); area.area()] }
    }

    /// Create a buffer covering `area`, filled with `cell`.
    pub fn filled(area: Rect, cell: Cell) -> Self {
        Buffer { area, cells: vec![cell; area.area()] }
    }

    /// The region this buffer covers.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Read-only view of all cells in row-major order.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    fn index_of(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.area.x || y < self.area.y || x >= self.area.right() || y >= self.area.bottom() {
            return None;
        }
        let col = (x - self.area.x) as usize;
        let row = (y - self.area.y) as usize;
        Some(row * self.area.width as usize + col)
    }

    /// Immutable access to the cell at `(x, y)`.
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.index_of(x, y).map(|i| &self.cells[i])
    }

    /// Mutable access to the cell at `(x, y)`.
    pub fn get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        self.index_of(x, y).map(move |i| &mut self.cells[i])
    }

    /// Resize / reposition the buffer, clearing all cells to blank.
    pub fn resize(&mut self, area: Rect) {
        self.area = area;
        self.cells.clear();
        self.cells.resize(area.area(), Cell::blank());
    }

    /// Reset every cell to blank (keeps the area).
    pub fn reset(&mut self) {
        for c in &mut self.cells {
            c.reset();
        }
    }

    /// Fill `rect` (clipped to the area) with `cell`.
    pub fn fill(&mut self, rect: Rect, cell: &Cell) {
        let r = rect.intersection(self.area);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                if let Some(dst) = self.get_mut(x, y) {
                    *dst = cell.clone();
                }
            }
        }
    }

    /// Paint a single character at `(x, y)` with `style`.
    ///
    /// Wide characters automatically claim the following cell as a
    /// continuation. Zero-width combining marks attach to the previous cell.
    /// Returns the number of columns advanced (0 if clipped or zero-width).
    pub fn set_char(&mut self, x: u16, y: u16, c: char, style: Style) -> u16 {
        let w = char_width(c);
        if w == 0 {
            if x > self.area.x
                && let Some(prev) = self.get_mut(x - 1, y)
            {
                let mut s = String::from(prev.symbol());
                s.push(c);
                prev.set_symbol(&s);
            }
            return 0;
        }
        match self.get_mut(x, y) {
            Some(cell) => {
                cell.set_char(c);
                cell.set_style(style);
            }
            None => return 0,
        }
        if w == 2
            && let Some(next) = self.get_mut(x + 1, y)
        {
            next.set_char(' ');
            next.set_style(style);
            next.continuation = true;
        }
        w
    }

    /// Paint `text` starting at `(x, y)` with `style`, clipping at the right
    /// edge of `max_width` (or the buffer edge). Returns the ending column.
    ///
    /// Newlines are not interpreted — callers wanting multi-line output should
    /// split first. Wide characters and combining marks are handled.
    pub fn set_str(&mut self, x: u16, y: u16, text: &str, style: Style, max_width: u16) -> u16 {
        let mut cx = x;
        let limit = x.saturating_add(max_width);
        for c in text.chars() {
            if cx >= limit || cx >= self.area.right() {
                break;
            }
            let w = char_width(c);
            if w == 2 && cx + 1 >= limit {
                break;
            }
            let advanced = self.set_char(cx, y, c, style);
            cx = cx.saturating_add(advanced);
        }
        cx
    }

    /// Overlay another buffer onto this one at its own coordinates.
    ///
    /// This is the compositor primitive used to stack windows: paint each
    /// window's buffer, then blit them in z-order onto the screen buffer.
    pub fn blit(&mut self, src: &Buffer) {
        let area = src.area().intersection(self.area);
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if let (Some(s), Some(d)) = (src.get(x, y), self.get_mut(x, y)) {
                    *d = s.clone();
                }
            }
        }
    }

    /// The list of cells that differ from `previous`, as `(Point, &Cell)`.
    ///
    /// Both buffers must share the same [`area`](Buffer::area); if they do not,
    /// every non-continuation cell of `self` is reported (a full repaint).
    pub fn diff<'a>(&'a self, previous: &Buffer) -> Vec<(Point, &'a Cell)> {
        let mut out = Vec::new();
        if self.area != previous.area {
            for y in self.area.y..self.area.bottom() {
                for x in self.area.x..self.area.right() {
                    if let Some(c) = self.get(x, y)
                        && !c.continuation
                    {
                        out.push((Point::new(x, y), c));
                    }
                }
            }
            return out;
        }
        let width = self.area.width as usize;
        for (i, (cur, prev)) in self.cells.iter().zip(previous.cells.iter()).enumerate() {
            if cur != prev && !cur.continuation {
                let col = (i % width) as u16 + self.area.x;
                let row = (i / width) as u16 + self.area.y;
                out.push((Point::new(col, row), cur));
            }
        }
        out
    }
}

/// The display width of a character in terminal cells: 0, 1 or 2.
///
/// This is a compact, dependency-free approximation of Unicode East-Asian width
/// plus the common emoji/wide ranges. It treats control characters and most
/// combining marks as zero-width, common CJK / fullwidth / wide-emoji ranges as
/// two, and everything else as one. It is not a full UAX #11 implementation but
/// covers the vast majority of real text seen in terminals.
pub fn char_width(c: char) -> u16 {
    let cp = c as u32;
    if cp < 0x20 || (0x7f..0xa0).contains(&cp) {
        return 0;
    }
    if matches!(cp, 0x200b..=0x200f | 0x2028 | 0x2029 | 0x202a..=0x202e | 0x2060 | 0xfeff) {
        return 0;
    }
    if is_combining(cp) {
        return 0;
    }
    if is_wide(cp) {
        return 2;
    }
    1
}

fn is_combining(cp: u32) -> bool {
    matches!(cp,
        0x0300..=0x036f |
        0x0483..=0x0489 |
        0x0591..=0x05bd |
        0x0610..=0x061a |
        0x064b..=0x065f |
        0x0670 |
        0x1ab0..=0x1aff |
        0x1dc0..=0x1dff |
        0x20d0..=0x20ff |
        0xfe20..=0xfe2f
    )
}

fn is_wide(cp: u32) -> bool {
    matches!(cp,
        0x1100..=0x115f |
        0x2329..=0x232a |
        0x2e80..=0x303e |
        0x3041..=0x33ff |
        0x3400..=0x4dbf |
        0x4e00..=0x9fff |
        0xa000..=0xa4cf |
        0xac00..=0xd7a3 |
        0xf900..=0xfaff |
        0xfe10..=0xfe19 |
        0xfe30..=0xfe6f |
        0xff00..=0xff60 |
        0xffe0..=0xffe6 |
        0x1f300..=0x1f64f |
        0x1f900..=0x1f9ff |
        0x20000..=0x3fffd
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::style::{Color, Style};

    #[test]
    fn set_str_and_get() {
        let mut b = Buffer::empty(Rect::new(0, 0, 10, 2));
        let end = b.set_str(0, 0, "hi", Style::new().fg(Color::Red), 10);
        assert_eq!(end, 2);
        assert_eq!(b.get(0, 0).unwrap().symbol_char(), 'h');
        assert_eq!(b.get(1, 0).unwrap().symbol_char(), 'i');
        assert_eq!(b.get(0, 0).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn diff_reports_only_changes() {
        let a = Buffer::empty(Rect::new(0, 0, 4, 1));
        let mut b = a.clone();
        b.set_char(2, 0, 'x', Style::new());
        let d = b.diff(&a);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, Point::new(2, 0));
    }

    #[test]
    fn wide_char_claims_two_cells() {
        let mut b = Buffer::empty(Rect::new(0, 0, 4, 1));
        let adv = b.set_char(0, 0, '世', Style::new());
        assert_eq!(adv, 2);
        assert!(b.get(1, 0).unwrap().is_continuation());
    }

    #[test]
    fn combining_mark_attaches() {
        let mut b = Buffer::empty(Rect::new(0, 0, 4, 1));
        b.set_char(0, 0, 'e', Style::new());
        let adv = b.set_char(1, 0, '\u{0301}', Style::new()); // combining acute
        assert_eq!(adv, 0);
        assert_eq!(b.get(0, 0).unwrap().symbol(), "e\u{0301}");
    }

    #[test]
    fn clipping_is_safe() {
        let mut b = Buffer::empty(Rect::new(0, 0, 3, 1));
        let end = b.set_str(0, 0, "hello", Style::new(), 100);
        assert_eq!(end, 3);
        assert!(b.get(5, 0).is_none());
    }
}
