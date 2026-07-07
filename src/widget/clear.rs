//! [`Clear`]: blank a region so a popup or dialog can draw over other content.

use crate::buffer::{Buffer, Cell};
use crate::geom::Rect;
use crate::style::Style;
use crate::widget::Widget;

/// Resets every cell in its area to a blank cell.
///
/// Render this first when drawing a floating element (a dialog, menu or
/// tooltip) so whatever was underneath does not bleed through, then draw the
/// element into the same area. An optional [`style`](Clear::style) fills the
/// cleared region with a background.
#[derive(Debug, Clone, Copy, Default)]
pub struct Clear {
    style: Option<Style>,
}

impl Clear {
    /// A plain clear that resets cells to the terminal default.
    pub fn new() -> Self {
        Clear { style: None }
    }

    /// Fill the cleared region with a background style instead of the default.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }
}

impl Widget for Clear {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut cell = Cell::blank();
        if let Some(style) = self.style {
            cell.set_style(style);
        }
        buf.fill(area, &cell);
    }
}
