//! [`Gauge`]: a horizontal progress bar with sub-cell precision.

use crate::buffer::Buffer;
use crate::geom::Rect;
use crate::style::{Color, Style};
use crate::widget::block::Block;
use crate::widget::{align_offset, Alignment, Widget};
use alloc::string::String;

/// Fractional block glyphs from empty (1/8) to full, for smooth bar edges.
const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// A progress bar.
///
/// The bar fills left-to-right in proportion to [`ratio`](Gauge::ratio) (clamped
/// to `0.0..=1.0`), using eighth-width block glyphs so progress moves smoothly
/// rather than jumping a whole cell at a time. An optional label is drawn
/// centered over the bar.
#[derive(Debug, Clone)]
pub struct Gauge {
    ratio: f32,
    label: Option<String>,
    filled_style: Style,
    unfilled_style: Style,
    label_style: Style,
    block: Option<Block>,
}

impl Default for Gauge {
    fn default() -> Self {
        Gauge {
            ratio: 0.0,
            label: None,
            filled_style: Style::new().fg(Color::Black).bg(Color::Green),
            unfilled_style: Style::new().fg(Color::Gray).bg(Color::DarkGray),
            label_style: Style::new(),
            block: None,
        }
    }
}

impl Gauge {
    /// A new, empty gauge.
    pub fn new() -> Self {
        Gauge::default()
    }

    /// Set the fill fraction (clamped to `0.0..=1.0`).
    pub fn ratio(mut self, ratio: f32) -> Self {
        self.ratio = ratio.clamp(0.0, 1.0);
        self
    }

    /// Set the fill from an integer percentage (0–100).
    pub fn percent(mut self, percent: u16) -> Self {
        self.ratio = (percent.min(100) as f32) / 100.0;
        self
    }

    /// Set an explicit label; defaults to the percentage when unset.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Style of the filled portion.
    pub fn filled_style(mut self, style: Style) -> Self {
        self.filled_style = style;
        self
    }

    /// Style of the empty portion.
    pub fn unfilled_style(mut self, style: Style) -> Self {
        self.unfilled_style = style;
        self
    }

    /// Wrap the gauge in a [`Block`].
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }
}

impl Widget for Gauge {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let area = match &self.block {
            Some(block) => {
                block.render(area, buf);
                block.inner(area)
            }
            None => area,
        };
        if area.is_empty() {
            return;
        }

        let width = area.width;
        // Total eighths of fill across the whole bar.
        // `f32::round` lives in std, not core; add 0.5 and truncate (ratio ≥ 0).
        let total_eighths = (self.ratio * (width as f32) * 8.0 + 0.5) as u32;
        let full = (total_eighths / 8) as u16;
        let partial = (total_eighths % 8) as usize;

        let row_mid = area.y + area.height / 2;
        for y in area.y..area.bottom() {
            for col in 0..width {
                let x = area.x + col;
                if col < full {
                    buf.set_char(x, y, '█', self.filled_style);
                } else if col == full && partial > 0 {
                    // Partial cell: draw the fractional glyph in the filled fg
                    // over the unfilled bg for a clean seam.
                    let style = Style {
                        fg: self.filled_style.bg.or(self.filled_style.fg),
                        bg: self.unfilled_style.bg,
                        attributes: self.filled_style.attributes,
                    };
                    buf.set_char(x, y, EIGHTHS[partial - 1], style);
                } else {
                    buf.set_char(x, y, ' ', self.unfilled_style);
                }
            }
        }

        // Label, centered on the middle row, styled to stay legible over both
        // the filled and empty regions.
        let owned;
        let label: &str = match &self.label {
            Some(l) => l,
            None => {
                owned = percent_string(self.ratio);
                &owned
            }
        };
        if !label.is_empty() {
            let lw = label.chars().map(crate::buffer::char_width).sum::<u16>().min(width);
            let start = area.x + align_offset(Alignment::Center, width, lw);
            // Draw each label char, choosing a legible fg against whatever the
            // bar painted underneath.
            let mut x = start;
            for ch in label.chars() {
                if x >= area.right() {
                    break;
                }
                let on_fill = (x - area.x) < full || ((x - area.x) == full && partial > 0);
                let base = if on_fill { self.filled_style } else { self.unfilled_style };
                let style = Style {
                    fg: self.label_style.fg.or(base.fg),
                    bg: base.bg,
                    attributes: base.attributes | self.label_style.attributes,
                };
                let adv = buf.set_char(x, row_mid, ch, style);
                x = x.saturating_add(adv.max(1));
            }
        }
    }
}

fn percent_string(ratio: f32) -> String {
    let pct = (ratio * 100.0 + 0.5) as u32;
    let mut s = String::new();
    // Simple integer-to-string to avoid pulling in float formatting machinery.
    let mut digits = [0u8; 3];
    let mut n = pct.min(100);
    let mut i = digits.len();
    if n == 0 {
        s.push('0');
    }
    while n > 0 {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    for &d in &digits[i..] {
        s.push(d as char);
    }
    s.push('%');
    s
}
