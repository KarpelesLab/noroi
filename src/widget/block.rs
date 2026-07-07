//! [`Block`]: a bordered, optionally-titled frame around other content.

use crate::buffer::Buffer;
use crate::geom::Rect;
use crate::style::Style;
use crate::widget::text::Line;
use crate::widget::{align_offset, Alignment, Widget};

/// Which edges of a [`Block`] draw a border, as a bitset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Borders(u8);

impl Borders {
    /// No border.
    pub const NONE: Borders = Borders(0);
    /// The top edge.
    pub const TOP: Borders = Borders(1 << 0);
    /// The right edge.
    pub const RIGHT: Borders = Borders(1 << 1);
    /// The bottom edge.
    pub const BOTTOM: Borders = Borders(1 << 2);
    /// The left edge.
    pub const LEFT: Borders = Borders(1 << 3);
    /// All four edges.
    pub const ALL: Borders = Borders(0b1111);

    /// True when every edge in `other` is set.
    pub const fn contains(self, other: Borders) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl core::ops::BitOr for Borders {
    type Output = Borders;
    fn bitor(self, rhs: Borders) -> Borders {
        Borders(self.0 | rhs.0)
    }
}

/// The line style used to draw a [`Block`]'s border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderType {
    /// Single light lines: `┌─┐│└┘`.
    #[default]
    Plain,
    /// Single light lines with rounded corners: `╭─╮│╰╯`.
    Rounded,
    /// Double lines: `╔═╗║╚╝`.
    Double,
    /// Heavy lines: `┏━┓┃┗┛`.
    Thick,
}

struct BorderChars {
    horizontal: char,
    vertical: char,
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
}

impl BorderType {
    fn chars(self) -> BorderChars {
        match self {
            BorderType::Plain => BorderChars {
                horizontal: '─',
                vertical: '│',
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
            },
            BorderType::Rounded => BorderChars {
                horizontal: '─',
                vertical: '│',
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
            },
            BorderType::Double => BorderChars {
                horizontal: '═',
                vertical: '║',
                top_left: '╔',
                top_right: '╗',
                bottom_left: '╚',
                bottom_right: '╝',
            },
            BorderType::Thick => BorderChars {
                horizontal: '━',
                vertical: '┃',
                top_left: '┏',
                top_right: '┓',
                bottom_left: '┗',
                bottom_right: '┛',
            },
        }
    }
}

/// Interior padding, in cells, between a [`Block`]'s border and its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Padding {
    /// Cells of padding on the left.
    pub left: u16,
    /// Cells of padding on the right.
    pub right: u16,
    /// Cells of padding on top.
    pub top: u16,
    /// Cells of padding on the bottom.
    pub bottom: u16,
}

impl Padding {
    /// Equal padding on all four sides.
    pub const fn uniform(n: u16) -> Self {
        Padding { left: n, right: n, top: n, bottom: n }
    }

    /// Separate horizontal and vertical padding.
    pub const fn symmetric(horizontal: u16, vertical: u16) -> Self {
        Padding { left: horizontal, right: horizontal, top: vertical, bottom: vertical }
    }
}

/// A frame: borders on any subset of edges, an optional title, a background
/// fill and interior padding. Call [`Block::inner`] to get the region left for
/// content after the border and padding are accounted for.
#[derive(Debug, Clone, Default)]
pub struct Block {
    borders: Borders,
    border_type: BorderType,
    border_style: Style,
    title: Option<Line>,
    title_alignment: Alignment,
    style: Style,
    padding: Padding,
}

impl Block {
    /// An empty block with no borders.
    pub fn new() -> Self {
        Block::default()
    }

    /// A block bordered on all four edges.
    pub fn bordered() -> Self {
        Block::new().borders(Borders::ALL)
    }

    /// Choose which edges draw a border.
    pub fn borders(mut self, borders: Borders) -> Self {
        self.borders = borders;
        self
    }

    /// Choose the border line style.
    pub fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    /// Style applied to the border characters.
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Set the title shown on the top border.
    pub fn title(mut self, title: impl Into<Line>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the title's horizontal alignment.
    pub fn title_alignment(mut self, alignment: Alignment) -> Self {
        self.title_alignment = alignment;
        self
    }

    /// Background/base style, applied to the whole block area.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Interior padding between border and content.
    pub fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }

    /// The content region remaining inside the borders and padding.
    pub fn inner(&self, area: Rect) -> Rect {
        let mut x = area.x;
        let mut y = area.y;
        let mut w = area.width;
        let mut h = area.height;
        if self.borders.contains(Borders::LEFT) {
            x = x.saturating_add(1);
            w = w.saturating_sub(1);
        }
        if self.borders.contains(Borders::RIGHT) {
            w = w.saturating_sub(1);
        }
        if self.borders.contains(Borders::TOP) {
            y = y.saturating_add(1);
            h = h.saturating_sub(1);
        }
        if self.borders.contains(Borders::BOTTOM) {
            h = h.saturating_sub(1);
        }
        x = x.saturating_add(self.padding.left);
        y = y.saturating_add(self.padding.top);
        w = w.saturating_sub(self.padding.left.saturating_add(self.padding.right));
        h = h.saturating_sub(self.padding.top.saturating_add(self.padding.bottom));
        Rect::new(x, y, w, h)
    }
}

impl Widget for Block {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        // Background fill.
        if self.style != Style::RESET {
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    if let Some(cell) = buf.get_mut(x, y) {
                        cell.patch_style(self.style);
                    }
                }
            }
        }

        let c = self.border_type.chars();
        let bs = self.border_style;
        let left = self.borders.contains(Borders::LEFT);
        let right = self.borders.contains(Borders::RIGHT);
        let top = self.borders.contains(Borders::TOP);
        let bottom = self.borders.contains(Borders::BOTTOM);
        let last_col = area.right().saturating_sub(1);
        let last_row = area.bottom().saturating_sub(1);

        if top {
            for x in area.x..area.right() {
                buf.set_char(x, area.y, c.horizontal, bs);
            }
        }
        if bottom {
            for x in area.x..area.right() {
                buf.set_char(x, last_row, c.horizontal, bs);
            }
        }
        if left {
            for y in area.y..area.bottom() {
                buf.set_char(area.x, y, c.vertical, bs);
            }
        }
        if right {
            for y in area.y..area.bottom() {
                buf.set_char(last_col, y, c.vertical, bs);
            }
        }
        if top && left {
            buf.set_char(area.x, area.y, c.top_left, bs);
        }
        if top && right {
            buf.set_char(last_col, area.y, c.top_right, bs);
        }
        if bottom && left {
            buf.set_char(area.x, last_row, c.bottom_left, bs);
        }
        if bottom && right {
            buf.set_char(last_col, last_row, c.bottom_right, bs);
        }

        // Title on the top edge, inset by one cell past the corners.
        if let Some(title) = &self.title
            && area.width > 2
        {
            let avail = area.width.saturating_sub(2);
            let tw = title.width().min(avail);
            let off = align_offset(self.title_alignment, avail, tw);
            let mut tx = area.x + 1 + off;
            let ty = area.y;
            let limit = area.x + 1 + avail;
            for span in &title.spans {
                if tx >= limit {
                    break;
                }
                let style = bs.patch(span.style);
                let end = buf.set_str(tx, ty, &span.content, style, limit - tx);
                tx = end;
            }
        }
    }
}
