//! [`Spinner`]: a frame-based "busy" indicator for indeterminate activity.

use crate::buffer::{Buffer, char_width};
use crate::geom::Rect;
use crate::style::Style;
use crate::widget::Widget;
use alloc::string::String;

/// An animated spinner cycling through a set of frames.
///
/// Unlike a [`Gauge`](crate::widget::Gauge), a spinner shows that *something is
/// happening* without knowing how far along it is. It carries its own animation
/// state: call [`advance`](Spinner::advance) with the elapsed seconds each frame
/// (the app owns the clock, since the core has none), then render it.
///
/// ```
/// use noroi::widget::Spinner;
/// let mut s = Spinner::new().label("Loading");
/// s.advance(0.1);         // step the animation
/// assert!(!s.frame().is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct Spinner {
    frames: &'static [&'static str],
    interval: f32,
    elapsed: f32,
    index: usize,
    style: Style,
    label: Option<String>,
    label_style: Style,
}

impl Spinner {
    /// Braille dots — the smooth default.
    pub const DOTS: &'static [&'static str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    /// The classic ASCII spinner (works on any terminal).
    pub const LINE: &'static [&'static str] = &["|", "/", "-", "\\"];
    /// A rotating arc.
    pub const ARC: &'static [&'static str] = &["◜", "◠", "◝", "◞", "◡", "◟"];
    /// A quartered circle.
    pub const CIRCLE: &'static [&'static str] = &["◐", "◓", "◑", "◒"];
    /// A bouncing vertical bar.
    pub const BAR: &'static [&'static str] = &[
        "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃", "▂",
    ];

    /// A new spinner using [`DOTS`](Spinner::DOTS) at ~12 fps.
    pub fn new() -> Self {
        Spinner {
            frames: Spinner::DOTS,
            interval: 0.08,
            elapsed: 0.0,
            index: 0,
            style: Style::RESET,
            label: None,
            label_style: Style::RESET,
        }
    }

    /// Use a specific frame set (one of the presets, or your own).
    pub fn frames(mut self, frames: &'static [&'static str]) -> Self {
        self.frames = if frames.is_empty() {
            Spinner::DOTS
        } else {
            frames
        };
        self.index %= self.frames.len();
        self
    }

    /// Seconds each frame is shown (smaller = faster).
    pub fn interval(mut self, seconds: f32) -> Self {
        self.interval = seconds.max(0.0);
        self
    }

    /// Style applied to the spinner glyph.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// A label drawn after the spinner glyph.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Style for the label (defaults to the glyph style).
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Advance the animation by `dt` seconds, stepping frames as needed.
    pub fn advance(&mut self, dt: f32) {
        if self.interval <= 0.0 {
            return;
        }
        self.elapsed += dt.max(0.0);
        while self.elapsed >= self.interval {
            self.elapsed -= self.interval;
            self.index = (self.index + 1) % self.frames.len();
        }
    }

    /// The current frame string.
    pub fn frame(&self) -> &str {
        self.frames[self.index]
    }

    /// The total rendered width (glyph plus `" " + label`).
    pub fn width(&self) -> u16 {
        let glyph: u16 = self.frame().chars().map(char_width).sum();
        match &self.label {
            Some(l) => glyph + 1 + l.chars().map(char_width).sum::<u16>(),
            None => glyph,
        }
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Spinner::new()
    }
}

impl Widget for Spinner {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let mut x = buf.set_str(area.x, area.y, self.frame(), self.style, area.width);
        if let Some(label) = &self.label {
            let remaining = area.right().saturating_sub(x);
            if remaining > 1 {
                x = buf.set_str(x, area.y, " ", self.label_style, remaining);
                buf.set_str(
                    x,
                    area.y,
                    label,
                    self.label_style,
                    area.right().saturating_sub(x),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_and_wraps() {
        let mut s = Spinner::new().frames(Spinner::LINE).interval(0.1);
        assert_eq!(s.frame(), "|");
        s.advance(0.1);
        assert_eq!(s.frame(), "/");
        s.advance(0.25); // skip two more frames
        assert_eq!(s.frame(), "\\");
        s.advance(0.1); // wrap back to start
        assert_eq!(s.frame(), "|");
    }

    #[test]
    fn zero_interval_holds_still() {
        let mut s = Spinner::new().interval(0.0);
        let f = s.frame().to_string();
        s.advance(100.0);
        assert_eq!(s.frame(), f);
    }
}
