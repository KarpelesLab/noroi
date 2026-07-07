//! Colors and text attributes.
//!
//! A [`Style`] pairs an optional foreground and background [`Color`] with a set
//! of [`Attributes`]. Styles compose: [`Style::patch`] overlays a second style,
//! taking its colors where they are set and unioning attributes. This lets a
//! widget declare a base style and override pieces of it without caring what the
//! surrounding context chose.

use core::fmt;

/// A terminal color.
///
/// The variants mirror the three color depths terminals support. A backend
/// downgrades gracefully: on a 16-color terminal a [`Color::Rgb`] is mapped to
/// the nearest indexed color, and so on. [`Color::Reset`] restores the
/// terminal's own default (distinct from any specific color).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// Use the terminal's configured default color.
    #[default]
    Reset,
    /// Black (ANSI 0).
    Black,
    /// Red (ANSI 1).
    Red,
    /// Green (ANSI 2).
    Green,
    /// Yellow (ANSI 3).
    Yellow,
    /// Blue (ANSI 4).
    Blue,
    /// Magenta (ANSI 5).
    Magenta,
    /// Cyan (ANSI 6).
    Cyan,
    /// White / light grey (ANSI 7).
    Gray,
    /// Bright black / dark grey (ANSI 8).
    DarkGray,
    /// Bright red (ANSI 9).
    LightRed,
    /// Bright green (ANSI 10).
    LightGreen,
    /// Bright yellow (ANSI 11).
    LightYellow,
    /// Bright blue (ANSI 12).
    LightBlue,
    /// Bright magenta (ANSI 13).
    LightMagenta,
    /// Bright cyan (ANSI 14).
    LightCyan,
    /// Bright white (ANSI 15).
    White,
    /// A color from the 256-color palette.
    Indexed(u8),
    /// A 24-bit true color.
    Rgb(u8, u8, u8),
}

impl Color {
    /// Map any color onto the 0-15 basic ANSI palette (best effort).
    ///
    /// [`Color::Reset`] returns `9`, but callers must handle reset separately —
    /// the sequence builder emits `39`/`49` for it rather than a palette index.
    pub fn to_ansi16(self) -> u8 {
        match self {
            Color::Reset => 9,
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::Gray => 7,
            Color::DarkGray => 8,
            Color::LightRed => 9,
            Color::LightGreen => 10,
            Color::LightYellow => 11,
            Color::LightBlue => 12,
            Color::LightMagenta => 13,
            Color::LightCyan => 14,
            Color::White => 15,
            Color::Indexed(i) => index_to_ansi16(i),
            Color::Rgb(r, g, b) => rgb_to_ansi16(r, g, b),
        }
    }

    /// Map any color onto the 256-color palette (best effort).
    pub fn to_indexed(self) -> u8 {
        match self {
            Color::Rgb(r, g, b) => rgb_to_index256(r, g, b),
            Color::Indexed(i) => i,
            Color::Reset => 7,
            other => other.to_ansi16(),
        }
    }
}

fn index_to_ansi16(i: u8) -> u8 {
    match i {
        0..=15 => i,
        232..=255 => {
            let level = i - 232;
            if level < 6 {
                0
            } else if level < 12 {
                8
            } else if level < 18 {
                7
            } else {
                15
            }
        }
        _ => {
            let c = i - 16;
            let r = c / 36;
            let g = (c % 36) / 6;
            let b = c % 6;
            cube_to_ansi16(r, g, b)
        }
    }
}

fn cube_to_ansi16(r: u8, g: u8, b: u8) -> u8 {
    let bright = r.max(g).max(b) >= 3;
    let bit = |v: u8| if v >= 3 { 1u8 } else { 0 };
    let base = bit(r) | (bit(g) << 1) | (bit(b) << 2);
    if base == 0 {
        if bright { 8 } else { 0 }
    } else if bright {
        base + 8
    } else {
        base
    }
}

fn rgb_to_ansi16(r: u8, g: u8, b: u8) -> u8 {
    let q = |v: u8| (v as u16 * 5 / 255) as u8;
    cube_to_ansi16(q(r), q(g), q(b))
}

fn rgb_to_index256(r: u8, g: u8, b: u8) -> u8 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max - min < 8 {
        let level = ((r as u16 + g as u16 + b as u16) / 3) as u8;
        if level < 8 {
            16
        } else if level > 248 {
            231
        } else {
            232 + ((level as u16 - 8) * 24 / 240) as u8
        }
    } else {
        let q = |v: u8| (v as u16 * 5 / 255) as u8;
        16 + 36 * q(r) + 6 * q(g) + q(b)
    }
}

/// Text rendering attributes, stored as a compact bitset.
///
/// Attributes are combined with the `|` operator and tested with
/// [`Attributes::contains`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Attributes(u16);

impl Attributes {
    /// No attributes.
    pub const NONE: Attributes = Attributes(0);
    /// Bold / increased intensity.
    pub const BOLD: Attributes = Attributes(1 << 0);
    /// Dim / decreased intensity.
    pub const DIM: Attributes = Attributes(1 << 1);
    /// Italic.
    pub const ITALIC: Attributes = Attributes(1 << 2);
    /// Underline.
    pub const UNDERLINE: Attributes = Attributes(1 << 3);
    /// Slow blink.
    pub const BLINK: Attributes = Attributes(1 << 4);
    /// Reverse video (swap fg/bg).
    pub const REVERSE: Attributes = Attributes(1 << 5);
    /// Concealed / hidden.
    pub const HIDDEN: Attributes = Attributes(1 << 6);
    /// Crossed out.
    pub const STRIKETHROUGH: Attributes = Attributes(1 << 7);

    /// True when this set contains every attribute in `other`.
    pub const fn contains(self, other: Attributes) -> bool {
        (self.0 & other.0) == other.0
    }

    /// True when no attribute is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Add the attributes in `other`.
    pub const fn insert(self, other: Attributes) -> Attributes {
        Attributes(self.0 | other.0)
    }

    /// Remove the attributes in `other`.
    pub const fn remove(self, other: Attributes) -> Attributes {
        Attributes(self.0 & !other.0)
    }

    /// The raw bitset (useful for backends).
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Reconstruct an attribute set from a raw bitset (unknown bits ignored).
    pub const fn from_bits(bits: u16) -> Attributes {
        Attributes(bits & 0xff)
    }
}

impl core::ops::BitOr for Attributes {
    type Output = Attributes;
    fn bitor(self, rhs: Attributes) -> Attributes {
        Attributes(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for Attributes {
    fn bitor_assign(&mut self, rhs: Attributes) {
        self.0 |= rhs.0;
    }
}

impl fmt::Debug for Attributes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [
            (Self::BOLD, "BOLD"),
            (Self::DIM, "DIM"),
            (Self::ITALIC, "ITALIC"),
            (Self::UNDERLINE, "UNDERLINE"),
            (Self::BLINK, "BLINK"),
            (Self::REVERSE, "REVERSE"),
            (Self::HIDDEN, "HIDDEN"),
            (Self::STRIKETHROUGH, "STRIKETHROUGH"),
        ];
        f.write_str("Attributes(")?;
        let mut first = true;
        for (attr, name) in names {
            if self.contains(attr) {
                if !first {
                    f.write_str(" | ")?;
                }
                first = false;
                f.write_str(name)?;
            }
        }
        if first {
            f.write_str("NONE")?;
        }
        f.write_str(")")
    }
}

/// A full cell style: foreground, background and attributes.
///
/// A `None` color means "leave whatever was there" when patching, and "the
/// terminal default" ([`Color::Reset`]) when finally rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Style {
    /// Foreground color, or `None` to inherit / default.
    pub fg: Option<Color>,
    /// Background color, or `None` to inherit / default.
    pub bg: Option<Color>,
    /// Attribute bitset.
    pub attributes: Attributes,
}

impl Style {
    /// An empty style that changes nothing.
    pub const RESET: Style = Style {
        fg: None,
        bg: None,
        attributes: Attributes::NONE,
    };

    /// A fresh, empty style (equivalent to [`Style::RESET`]).
    pub const fn new() -> Self {
        Style::RESET
    }

    /// Set the foreground color.
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Set the background color.
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Add attributes.
    pub const fn attrs(mut self, attributes: Attributes) -> Self {
        self.attributes = self.attributes.insert(attributes);
        self
    }

    /// Convenience: add [`Attributes::BOLD`].
    pub const fn bold(self) -> Self {
        self.attrs(Attributes::BOLD)
    }

    /// Convenience: add [`Attributes::ITALIC`].
    pub const fn italic(self) -> Self {
        self.attrs(Attributes::ITALIC)
    }

    /// Convenience: add [`Attributes::UNDERLINE`].
    pub const fn underline(self) -> Self {
        self.attrs(Attributes::UNDERLINE)
    }

    /// Convenience: add [`Attributes::REVERSE`].
    pub const fn reverse(self) -> Self {
        self.attrs(Attributes::REVERSE)
    }

    /// Convenience: add [`Attributes::DIM`].
    pub const fn dim(self) -> Self {
        self.attrs(Attributes::DIM)
    }

    /// Overlay `other` on top of `self`.
    ///
    /// Colors present in `other` replace those in `self`; attributes are
    /// unioned. Used to let widgets layer a partial style over a base.
    pub fn patch(self, other: Style) -> Style {
        Style {
            fg: other.fg.or(self.fg),
            bg: other.bg.or(self.bg),
            attributes: self.attributes | other.attributes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_overlays() {
        let base = Style::new().fg(Color::Red).bold();
        let over = Style::new().fg(Color::Green).italic();
        let merged = base.patch(over);
        assert_eq!(merged.fg, Some(Color::Green));
        assert!(merged.attributes.contains(Attributes::BOLD));
        assert!(merged.attributes.contains(Attributes::ITALIC));
    }

    #[test]
    fn rgb_downgrade() {
        assert_eq!(Color::Rgb(0, 0, 0).to_ansi16(), 0);
        assert_eq!(Color::Rgb(255, 255, 255).to_ansi16(), 15);
        assert_eq!(Color::Rgb(255, 0, 0).to_ansi16(), 9);
        let idx = Color::Rgb(128, 128, 128).to_indexed();
        assert!((232..=255).contains(&idx));
    }
}
