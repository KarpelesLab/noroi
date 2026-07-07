//! Terminal capabilities and escape-sequence generation.
//!
//! noroi does not ship a terminfo database. Modern terminals are overwhelmingly
//! xterm-compatible, so instead of parsing capability files this module models
//! the handful of axes that actually differ in practice — chiefly color depth —
//! in [`Caps`], and emits standard ECMA-48 / xterm control sequences.
//!
//! Sequences are written into a caller-owned `Vec<u8>` so a backend can batch a
//! whole frame into one write. Colors are downgraded to the terminal's declared
//! [`ColorDepth`] automatically.

use crate::style::{Attributes, Color, Style};
use alloc::vec::Vec;

/// How many colors the terminal can display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// No color (monochrome); colors are dropped, attributes kept.
    None,
    /// The 16 basic ANSI colors.
    Ansi16,
    /// The 256-color palette.
    Indexed256,
    /// 24-bit true color.
    TrueColor,
}

/// A terminal's relevant capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// Color depth to target when emitting SGR.
    pub colors: ColorDepth,
    /// Whether the terminal is believed to support mouse reporting.
    pub mouse: bool,
    /// Whether the terminal supports bracketed paste.
    pub bracketed_paste: bool,
    /// Whether the terminal supports focus in/out reporting.
    pub focus: bool,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            colors: ColorDepth::Ansi16,
            mouse: true,
            bracketed_paste: true,
            focus: true,
        }
    }
}

impl Caps {
    /// Infer capabilities from the `TERM` and `COLORTERM` environment values.
    ///
    /// This mirrors what libraries and shells commonly do: `COLORTERM` of
    /// `truecolor`/`24bit` implies true color; a `TERM` containing `256color`
    /// implies the 256-color palette; `dumb` or absent implies no color.
    pub fn detect(term: Option<&str>, colorterm: Option<&str>) -> Caps {
        let colors = if matches!(colorterm, Some(c) if c.contains("truecolor") || c.contains("24bit"))
        {
            ColorDepth::TrueColor
        } else {
            match term {
                None => ColorDepth::None,
                Some(t) if t == "dumb" || t.is_empty() => ColorDepth::None,
                Some(t) if t.contains("256color") || t.contains("256") => ColorDepth::Indexed256,
                Some(t)
                    if t.contains("color")
                        || t.contains("xterm")
                        || t.contains("screen")
                        || t.contains("tmux") =>
                {
                    ColorDepth::Ansi16
                }
                Some(_) => ColorDepth::Ansi16,
            }
        };
        let dumb = matches!(term, None | Some("dumb"));
        Caps {
            colors,
            mouse: !dumb,
            bracketed_paste: !dumb,
            focus: !dumb,
        }
    }
}

const ESC: u8 = 0x1b;

fn push_int(out: &mut Vec<u8>, mut v: u32) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&tmp[i..]);
}

/// Move the cursor to `(x, y)` (both zero-based); emits `CSI y+1 ; x+1 H`.
pub fn move_to(out: &mut Vec<u8>, x: u16, y: u16) {
    out.push(ESC);
    out.push(b'[');
    push_int(out, y as u32 + 1);
    out.push(b';');
    push_int(out, x as u32 + 1);
    out.push(b'H');
}

/// Hide the cursor (`CSI ? 25 l`).
pub fn hide_cursor(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?25l");
}

/// Show the cursor (`CSI ? 25 h`).
pub fn show_cursor(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?25h");
}

/// Clear the whole screen and home the cursor.
pub fn clear_screen(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[2J\x1b[H");
}

/// Clear from the cursor to the end of the line.
pub fn clear_to_line_end(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[K");
}

/// Enter the alternate screen buffer (`CSI ? 1049 h`).
pub fn enter_alternate_screen(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?1049h");
}

/// Leave the alternate screen buffer (`CSI ? 1049 l`).
pub fn leave_alternate_screen(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?1049l");
}

/// Enable mouse reporting: button, drag and motion, in SGR (1006) encoding.
pub fn enable_mouse(out: &mut Vec<u8>) {
    // 1000 button, 1002 button+drag, 1003 any-motion, 1006 SGR extended coords.
    out.extend_from_slice(b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h");
}

/// Disable mouse reporting (mirror of [`enable_mouse`]).
pub fn disable_mouse(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l");
}

/// Enable bracketed paste (`CSI ? 2004 h`).
pub fn enable_bracketed_paste(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?2004h");
}

/// Disable bracketed paste (`CSI ? 2004 l`).
pub fn disable_bracketed_paste(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?2004l");
}

/// Enable focus in/out reporting (`CSI ? 1004 h`).
pub fn enable_focus_reporting(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?1004h");
}

/// Disable focus in/out reporting (`CSI ? 1004 l`).
pub fn disable_focus_reporting(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[?1004l");
}

/// Reset all SGR attributes and colors (`CSI 0 m`).
pub fn reset_sgr(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[0m");
}

/// Emit the SGR needed to move the terminal's current state from `from` to `to`.
///
/// For simplicity and correctness this resets and re-applies whenever the styles
/// differ; the caller (the renderer) only calls this when a run of cells shares
/// a style, so the cost is amortized. Colors are downgraded to `depth`.
pub fn write_style(out: &mut Vec<u8>, from: &Style, to: &Style, depth: ColorDepth) {
    if from == to {
        return;
    }
    out.push(ESC);
    out.push(b'[');
    // Start with a full reset so we never leak a stale attribute.
    out.push(b'0');

    let attrs = to.attributes;
    let push_code = |code: &[u8], out: &mut Vec<u8>| {
        out.push(b';');
        out.extend_from_slice(code);
    };
    if attrs.contains(Attributes::BOLD) {
        push_code(b"1", out);
    }
    if attrs.contains(Attributes::DIM) {
        push_code(b"2", out);
    }
    if attrs.contains(Attributes::ITALIC) {
        push_code(b"3", out);
    }
    if attrs.contains(Attributes::UNDERLINE) {
        push_code(b"4", out);
    }
    if attrs.contains(Attributes::BLINK) {
        push_code(b"5", out);
    }
    if attrs.contains(Attributes::REVERSE) {
        push_code(b"7", out);
    }
    if attrs.contains(Attributes::HIDDEN) {
        push_code(b"8", out);
    }
    if attrs.contains(Attributes::STRIKETHROUGH) {
        push_code(b"9", out);
    }
    out.push(b'm');

    if let Some(fg) = to.fg {
        write_color(out, fg, true, depth);
    }
    if let Some(bg) = to.bg {
        write_color(out, bg, false, depth);
    }
}

/// Emit an SGR color sequence for `color` as foreground (`fg = true`) or
/// background, downgraded to `depth`.
fn write_color(out: &mut Vec<u8>, color: Color, fg: bool, depth: ColorDepth) {
    if depth == ColorDepth::None {
        return;
    }
    if color == Color::Reset {
        out.push(ESC);
        out.push(b'[');
        push_int(out, if fg { 39 } else { 49 });
        out.push(b'm');
        return;
    }
    match depth {
        ColorDepth::None => {}
        ColorDepth::TrueColor => match color {
            Color::Rgb(r, g, b) => {
                out.push(ESC);
                out.push(b'[');
                push_int(out, if fg { 38 } else { 48 });
                out.extend_from_slice(b";2;");
                push_int(out, r as u32);
                out.push(b';');
                push_int(out, g as u32);
                out.push(b';');
                push_int(out, b as u32);
                out.push(b'm');
            }
            other => write_basic_or_indexed(out, other, fg, true),
        },
        ColorDepth::Indexed256 => {
            let idx = color.to_indexed();
            match color {
                c @ (Color::Rgb(..) | Color::Indexed(_)) => {
                    let _ = c;
                    out.push(ESC);
                    out.push(b'[');
                    push_int(out, if fg { 38 } else { 48 });
                    out.extend_from_slice(b";5;");
                    push_int(out, idx as u32);
                    out.push(b'm');
                }
                other => write_basic_or_indexed(out, other, fg, true),
            }
        }
        ColorDepth::Ansi16 => {
            let idx = color.to_ansi16();
            write_ansi16(out, idx, fg);
        }
    }
}

/// Write a basic (0-15) color, or fall back to indexed for 256-capable depths.
fn write_basic_or_indexed(out: &mut Vec<u8>, color: Color, fg: bool, allow256: bool) {
    match color {
        Color::Indexed(i) if allow256 => {
            out.push(ESC);
            out.push(b'[');
            push_int(out, if fg { 38 } else { 48 });
            out.extend_from_slice(b";5;");
            push_int(out, i as u32);
            out.push(b'm');
        }
        other => write_ansi16(out, other.to_ansi16(), fg),
    }
}

/// Write an SGR sequence for a basic ANSI color index (0-15).
fn write_ansi16(out: &mut Vec<u8>, idx: u8, fg: bool) {
    out.push(ESC);
    out.push(b'[');
    let code = if idx < 8 {
        if fg { 30 + idx as u32 } else { 40 + idx as u32 }
    } else if fg {
        90 + (idx - 8) as u32
    } else {
        100 + (idx - 8) as u32
    };
    push_int(out, code);
    out.push(b'm');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_depths() {
        assert_eq!(
            Caps::detect(Some("xterm-256color"), None).colors,
            ColorDepth::Indexed256
        );
        assert_eq!(
            Caps::detect(Some("xterm"), Some("truecolor")).colors,
            ColorDepth::TrueColor
        );
        assert_eq!(Caps::detect(Some("dumb"), None).colors, ColorDepth::None);
        assert_eq!(Caps::detect(None, None).colors, ColorDepth::None);
    }

    #[test]
    fn move_to_is_one_based() {
        let mut out = Vec::new();
        move_to(&mut out, 0, 0);
        assert_eq!(out, b"\x1b[1;1H");
        out.clear();
        move_to(&mut out, 4, 9);
        assert_eq!(out, b"\x1b[10;5H");
    }

    #[test]
    fn truecolor_fg() {
        let mut out = Vec::new();
        let to = Style::new().fg(Color::Rgb(10, 20, 30));
        write_style(&mut out, &Style::RESET, &to, ColorDepth::TrueColor);
        assert_eq!(out, b"\x1b[0m\x1b[38;2;10;20;30m");
    }

    #[test]
    fn ansi16_bg() {
        let mut out = Vec::new();
        let to = Style::new().bg(Color::Red);
        write_style(&mut out, &Style::RESET, &to, ColorDepth::Ansi16);
        assert_eq!(out, b"\x1b[0m\x1b[41m");
    }
}
