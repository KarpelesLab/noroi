//! [`Button`]: a focusable, clickable labelled control.

use crate::buffer::{char_width, Buffer};
use crate::geom::Rect;
use crate::style::{Attributes, Color, Style};
use crate::widget::{align_offset, Alignment, Widget};
use alloc::string::String;

/// A labelled button.
///
/// A button is purely presentational — it draws a label on a filled background
/// and, when [`focused`](Button::focused), swaps in a highlight style. Hit
/// testing (did a click land inside its `area`?) and focus tracking are the
/// caller's job, which keeps the widget usable in any event model.
#[derive(Debug, Clone)]
pub struct Button {
    label: String,
    style: Style,
    focus_style: Style,
    focused: bool,
    alignment: Alignment,
}

impl Button {
    /// A button with the given label and sensible default styling.
    pub fn new(label: impl Into<String>) -> Self {
        Button {
            label: label.into(),
            style: Style::new().fg(Color::Black).bg(Color::Gray),
            focus_style: Style::new().fg(Color::Black).bg(Color::LightBlue).attrs(Attributes::BOLD),
            focused: false,
            alignment: Alignment::Center,
        }
    }

    /// Style used when not focused.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Style used when focused.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Set the focused flag.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Label alignment within the button (default centered).
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
}

impl Widget for Button {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let style = if self.focused { self.focus_style } else { self.style };
        // Fill the whole button.
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if let Some(cell) = buf.get_mut(x, y) {
                    cell.set_char(' ');
                    cell.set_style(style);
                }
            }
        }
        // Center the label on the middle row.
        let label_width: u16 = self.label.chars().map(char_width).sum();
        let clipped = label_width.min(area.width);
        let x = area.x + align_offset(self.alignment, area.width, clipped);
        let y = area.y + area.height / 2;
        buf.set_str(x, y, &self.label, style, area.width);
    }
}
