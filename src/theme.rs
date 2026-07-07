//! Cohesive visual theming.
//!
//! Widgets take explicit [`Style`]s, which is flexible but leaves each app to
//! invent its own palette. A [`Theme`] collects the handful of *roles* a UI
//! actually has — text, dim text, an accent, borders, selection, and so on —
//! into one value you thread through your rendering, so a whole application
//! shares one identity and can be re-skinned by swapping a single theme.
//!
//! Two presets ship: [`Theme::ofuda`] (the default — sumi ink, washi paper and a
//! vermilion seal, after noroi's namesake 呪い) and [`Theme::mono`] (no color;
//! hierarchy comes from bold/dim/reverse, for monochrome or `TERM=dumb`).
//! Colors are true-color and downgrade automatically on 256/16-color terminals.
//!
//! ```
//! use noroi::theme::Theme;
//! use noroi::widget::Widget;
//! # use noroi::buffer::Buffer;
//! # use noroi::geom::Rect;
//! let theme = Theme::ofuda();
//! # let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
//! // A panel whose border thickens and turns vermilion when focused:
//! theme.panel(true).render(Rect::new(0, 0, 10, 3), &mut buf);
//! ```

use crate::style::{Attributes, Color, Style};
use crate::widget::{Block, BorderType, Borders};

/// A palette of styled roles shared across an application's widgets.
///
/// Fields are [`Style`]s (not bare colors) so a role can carry attributes too —
/// e.g. [`title`](Theme::title) is gold *and* bold. Construct one of the presets
/// and override individual roles as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The screen/base fill.
    pub background: Style,
    /// Primary body text.
    pub text: Style,
    /// Secondary / muted text (hints, captions, inactive labels).
    pub dim: Style,
    /// The single emphasis color (vermilion in the default theme).
    pub accent: Style,
    /// A secondary accent (gold), for titles and highlights.
    pub accent_alt: Style,
    /// Border of an unfocused panel.
    pub border: Style,
    /// Border of the focused panel.
    pub border_focused: Style,
    /// Panel / section titles.
    pub title: Style,
    /// A selected row or item — the "stamp".
    pub selection: Style,
    /// A resting button.
    pub button: Style,
    /// The focused button.
    pub button_focused: Style,
    /// The filled portion of a gauge.
    pub gauge_filled: Style,
    /// The empty portion of a gauge.
    pub gauge_unfilled: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::ofuda()
    }
}

impl Theme {
    /// The default theme: sumi ink, washi paper, a vermilion seal and gold.
    pub const fn ofuda() -> Self {
        // 朱 vermilion, 金 gold, 墨 sumi ink, washi paper.
        let ink = Color::Rgb(18, 16, 21);
        let ink_lift = Color::Rgb(38, 34, 42);
        let washi = Color::Rgb(222, 216, 201);
        let muted = Color::Rgb(124, 116, 110);
        let vermilion = Color::Rgb(193, 39, 45);
        let gold = Color::Rgb(197, 160, 89);
        let hairline = Color::Rgb(84, 78, 88);

        Theme {
            background: Style::new().bg(ink).fg(washi),
            text: Style::new().fg(washi),
            dim: Style::new().fg(muted),
            accent: Style::new().fg(vermilion).bold(),
            accent_alt: Style::new().fg(gold),
            border: Style::new().fg(hairline),
            border_focused: Style::new().fg(vermilion).bold(),
            title: Style::new().fg(gold).bold(),
            // The hanko stamp: washi on vermilion.
            selection: Style::new().fg(washi).bg(vermilion).bold(),
            button: Style::new().fg(washi).bg(ink_lift),
            button_focused: Style::new().fg(ink).bg(vermilion).bold(),
            gauge_filled: Style::new().fg(ink).bg(vermilion),
            gauge_unfilled: Style::new().fg(muted).bg(ink_lift),
        }
    }

    /// A colorless theme: all hierarchy from bold / dim / reverse. Suitable for
    /// monochrome terminals, `TERM=dumb`, and maximum legibility.
    pub const fn mono() -> Self {
        let rev = Style::new().attrs(Attributes::REVERSE);
        Theme {
            background: Style::RESET,
            text: Style::new(),
            dim: Style::new().dim(),
            accent: Style::new().bold(),
            accent_alt: Style::new().bold(),
            border: Style::new().dim(),
            border_focused: Style::new().bold(),
            title: Style::new().bold(),
            selection: rev,
            button: Style::new()
                .attrs(Attributes::REVERSE)
                .attrs(Attributes::DIM),
            button_focused: rev.attrs(Attributes::BOLD),
            gauge_filled: rev,
            gauge_unfilled: Style::new().dim(),
        }
    }

    /// The border style a panel should use given its focus state — thick and
    /// accented when focused, thin and quiet when not.
    pub fn panel_border(self, focused: bool) -> BorderType {
        if focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        }
    }

    /// A [`Block`] pre-styled for this theme: all-sides border whose weight and
    /// color track `focused`, filled with the theme background. Add a title and
    /// content at the call site.
    pub fn panel(self, focused: bool) -> Block {
        Block::bordered()
            .borders(Borders::ALL)
            .border_type(self.panel_border(focused))
            .border_style(if focused {
                self.border_focused
            } else {
                self.border
            })
            .style(self.background)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_differ_and_focus_thickens() {
        let t = Theme::ofuda();
        assert_eq!(t.panel_border(false), BorderType::Plain);
        assert_eq!(t.panel_border(true), BorderType::Thick);
        // The stamp really is reverse-ish: distinct fg/bg.
        assert_ne!(t.selection.fg, t.selection.bg);
        // Mono carries no color, only attributes.
        let m = Theme::mono();
        assert!(m.accent.fg.is_none());
        assert!(m.selection.attributes.contains(Attributes::REVERSE));
    }
}
