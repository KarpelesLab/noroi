//! [`List`]: a scrollable, selectable vertical list of items.

use crate::buffer::{char_width, Buffer};
use crate::geom::Rect;
use crate::style::{Color, Style};
use crate::widget::block::Block;
use crate::widget::text::Text;
use crate::widget::{StatefulWidget, Widget};
use alloc::string::String;
use alloc::vec::Vec;

/// One entry in a [`List`]. May span multiple rows if its [`Text`] has multiple
/// lines.
#[derive(Debug, Clone, Default)]
pub struct ListItem {
    content: Text,
    style: Style,
}

impl ListItem {
    /// An item from any text-like value.
    pub fn new(content: impl Into<Text>) -> Self {
        ListItem { content: content.into(), style: Style::RESET }
    }

    /// Per-item base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Height of this item in rows.
    pub fn height(&self) -> u16 {
        self.content.height().max(1)
    }
}

impl<T: Into<Text>> From<T> for ListItem {
    fn from(value: T) -> Self {
        ListItem::new(value)
    }
}

/// Selection and scroll position for a [`List`], owned by the caller across
/// frames.
#[derive(Debug, Clone, Default)]
pub struct ListState {
    offset: usize,
    selected: Option<usize>,
}

impl ListState {
    /// A fresh state with nothing selected.
    pub fn new() -> Self {
        ListState::default()
    }

    /// The currently selected index, if any.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The index of the first visible item.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Select an explicit index (or clear with `None`).
    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        if index.is_none() {
            self.offset = 0;
        }
    }

    /// Move selection to the next item, wrapping, within a list of `len` items.
    pub fn next(&mut self, len: usize) {
        if len == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            Some(i) if i + 1 < len => i + 1,
            Some(_) => 0,
            None => 0,
        });
    }

    /// Move selection to the previous item, wrapping, within `len` items.
    pub fn previous(&mut self, len: usize) {
        if len == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        });
    }
}

/// A vertical list of [`ListItem`]s with optional selection highlighting.
///
/// Render it with [`StatefulWidget::render_stateful`] and a caller-owned
/// [`ListState`] to keep selection and scroll across frames, or with the plain
/// [`Widget`] impl for a static, non-interactive list.
#[derive(Debug, Clone, Default)]
pub struct List {
    items: Vec<ListItem>,
    block: Option<Block>,
    style: Style,
    highlight_style: Style,
    highlight_symbol: String,
}

impl List {
    /// Build a list from items.
    pub fn new<I, T>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<ListItem>,
    {
        List {
            items: items.into_iter().map(Into::into).collect(),
            block: None,
            style: Style::RESET,
            highlight_style: Style::new().fg(Color::Black).bg(Color::LightBlue),
            highlight_symbol: String::from("> "),
        }
    }

    /// Wrap the list in a [`Block`].
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Base style applied to every row.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Style applied to the selected row.
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Symbol drawn to the left of the selected row (blanked for others so
    /// content stays aligned).
    pub fn highlight_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.highlight_symbol = symbol.into();
        self
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when the list has no items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn symbol_width(&self) -> u16 {
        self.highlight_symbol.chars().map(char_width).sum()
    }
}

impl StatefulWidget for List {
    type State = ListState;

    fn render_stateful(&self, area: Rect, buf: &mut Buffer, state: &mut ListState) {
        let area = match &self.block {
            Some(block) => {
                block.render(area, buf);
                block.inner(area)
            }
            None => area,
        };
        if area.is_empty() || self.items.is_empty() {
            return;
        }
        let visible_rows = area.height;
        let sym_w = self.symbol_width();

        // Scroll so the selected item is visible. Items are treated as their
        // own height; for scrolling we count in items, keeping it predictable.
        if let Some(sel) = state.selected {
            let sel = sel.min(self.items.len() - 1);
            if sel < state.offset {
                state.offset = sel;
            } else {
                // Walk forward from offset accumulating heights until `sel` fits.
                loop {
                    let mut used = 0u16;
                    let mut last_visible = state.offset;
                    for (i, item) in self.items.iter().enumerate().skip(state.offset) {
                        let h = item.height();
                        if used + h > visible_rows {
                            break;
                        }
                        used += h;
                        last_visible = i;
                    }
                    if sel <= last_visible || state.offset >= self.items.len() - 1 {
                        break;
                    }
                    state.offset += 1;
                }
            }
        }
        state.offset = state.offset.min(self.items.len().saturating_sub(1));

        let mut y = area.y;
        for (i, item) in self.items.iter().enumerate().skip(state.offset) {
            let h = item.height();
            if y >= area.bottom() {
                break;
            }
            let selected = state.selected == Some(i);
            let row_style = if selected {
                self.style.patch(item.style).patch(self.highlight_style)
            } else {
                self.style.patch(item.style)
            };

            // Fill the row background across the whole width when selected.
            if selected {
                for ry in y..(y + h).min(area.bottom()) {
                    for rx in area.x..area.right() {
                        if let Some(cell) = buf.get_mut(rx, ry) {
                            cell.set_char(' ');
                            cell.set_style(row_style);
                        }
                    }
                }
            }

            // Highlight symbol (or padding) in the gutter.
            let gutter = sym_w;
            if selected && sym_w > 0 {
                buf.set_str(area.x, y, &self.highlight_symbol, row_style, sym_w);
            }

            let text_x = area.x.saturating_add(gutter);
            let text_w = area.width.saturating_sub(gutter);
            for (line_idx, line) in item.content.lines.iter().enumerate() {
                let ly = y + line_idx as u16;
                if ly >= area.bottom() {
                    break;
                }
                let mut x = text_x;
                for span in &line.spans {
                    let style = row_style.patch(span.style);
                    let end = buf.set_str(x, ly, &span.content, style, text_w.saturating_sub(x - text_x));
                    x = end;
                }
            }

            y += h;
        }
    }
}

impl Widget for List {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut state = ListState::new();
        self.render_stateful(area, buf, &mut state);
    }
}
