//! Renderable widgets.
//!
//! A [`Widget`] paints itself into a region of a [`Buffer`]. Widgets are values
//! you construct and hand an `area`; they never touch the terminal. This module
//! provides the trait, shared enums, styled-text primitives ([`Span`], [`Line`],
//! [`Text`]) and a set of ready widgets:
//!
//! * [`Block`] — borders, titles and padding; the frame around other content.
//! * [`Paragraph`] — wrapped or clipped multi-line styled text.
//! * [`Button`] — a focusable, clickable label.
//! * [`Gauge`] — a progress bar with an optional label.
//! * [`List`] — a scrollable, selectable list (holds its own [`ListState`]).
//! * [`Clear`] — blanks a region (used behind popups and dialogs).
//!
//! Stateful widgets that need to persist selection or scroll between frames
//! implement [`StatefulWidget`] and take their state by `&mut`.

use crate::buffer::Buffer;
use crate::geom::Rect;

mod block;
mod button;
mod clear;
mod gauge;
mod list;
mod paragraph;
mod text;

pub use block::{Block, BorderType, Borders, Padding};
pub use button::Button;
pub use clear::Clear;
pub use gauge::Gauge;
pub use list::{List, ListItem, ListState};
pub use paragraph::{Paragraph, Wrap};
pub use text::{Line, Span, Text};

/// Horizontal alignment for text and titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    /// Align to the left edge (default).
    #[default]
    Left,
    /// Center within the region.
    Center,
    /// Align to the right edge.
    Right,
}

/// Something that can paint itself into `area` of a [`Buffer`].
///
/// Implementations must clip to `area` (the [`Buffer`] methods already ignore
/// out-of-bounds writes, so painting slightly past the edge is safe) and must
/// not assume `area` starts at the origin.
pub trait Widget {
    /// Paint into `area`.
    fn render(&self, area: Rect, buf: &mut Buffer);
}

/// A widget that renders against externally-owned, mutable state.
///
/// Use this for widgets whose scroll or selection must survive across frames —
/// the caller keeps the [`State`](StatefulWidget::State) and passes it in each
/// time. [`List`] is the canonical example.
pub trait StatefulWidget {
    /// The persistent state this widget reads and updates while rendering.
    type State;
    /// Paint into `area`, consulting and updating `state`.
    fn render_stateful(&self, area: Rect, buf: &mut Buffer, state: &mut Self::State);
}

/// Compute the starting column for `content_width` cells within `width` under
/// `alignment`. Saturates so it never underflows when content is too wide.
pub(crate) fn align_offset(alignment: Alignment, width: u16, content_width: u16) -> u16 {
    match alignment {
        Alignment::Left => 0,
        Alignment::Center => width.saturating_sub(content_width) / 2,
        Alignment::Right => width.saturating_sub(content_width),
    }
}
