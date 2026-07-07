//! Incremental parsing of a terminal input byte stream into [`Event`]s.
//!
//! Terminals report keys, mouse actions and other events as a mix of raw bytes
//! and ANSI escape sequences. [`Parser`] consumes bytes as they arrive — in any
//! chunking — and yields decoded [`Event`]s. It understands:
//!
//! * UTF-8 text and C0 control bytes (mapped to Ctrl-keyed [`KeyCode`]s).
//! * CSI sequences (`ESC [ …`): arrows, navigation, function keys, and the
//!   `1;mods` / `n;mods~` modified forms (Ctrl/Alt/Shift + key).
//! * SS3 sequences (`ESC O …`): application-mode arrows and F1–F4.
//! * SGR (`1006`) and legacy X10 mouse reports, including drag and wheel.
//! * Bracketed paste (`ESC [ 200 ~ … ESC [ 201 ~`).
//! * Focus in/out (`ESC [ I` / `ESC [ O`).
//! * `Alt`-prefixed keys (`ESC` followed by another key).
//!
//! ### The lone-Escape problem
//! A bare `ESC` byte is ambiguous: it might be the user pressing Escape, or the
//! first byte of a sequence still in flight. [`Parser::feed`] therefore leaves a
//! trailing incomplete escape buffered. When a backend's read times out with
//! bytes still pending, it calls [`Parser::flush`] to resolve them (a lone `ESC`
//! becomes [`KeyCode::Esc`]).

use crate::event::{Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind};
use alloc::string::String;
use alloc::vec::Vec;

/// A stateful, incremental terminal-input parser.
///
/// Feed it bytes with [`feed`](Parser::feed); collect the [`Event`]s it returns.
#[derive(Debug, Default)]
pub struct Parser {
    buf: Vec<u8>,
}

/// Outcome of trying to parse one event from the front of the buffer.
enum Step {
    /// Produced an event, consuming `used` bytes.
    Event(Event, usize),
    /// Consumed `used` bytes that produced nothing (e.g. a stray byte).
    Skip(usize),
    /// Not enough bytes yet; wait for more.
    Incomplete,
}

impl Parser {
    /// Create an empty parser.
    pub fn new() -> Self {
        Parser { buf: Vec::new() }
    }

    /// Feed freshly read bytes and return every complete event they yield.
    ///
    /// Any trailing incomplete sequence is retained for the next call.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            match parse_one(&self.buf, false) {
                Step::Event(ev, used) => {
                    self.buf.drain(..used);
                    out.push(ev);
                }
                Step::Skip(used) => {
                    self.buf.drain(..used);
                }
                Step::Incomplete => break,
            }
        }
        out
    }

    /// Resolve any buffered, incomplete sequence.
    ///
    /// Call this when a read has timed out and no more bytes are coming for now:
    /// a pending lone `ESC` is emitted as [`KeyCode::Esc`], and any other stuck
    /// prefix is force-parsed as best it can be.
    pub fn flush(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            if self.buf.is_empty() {
                break;
            }
            match parse_one(&self.buf, true) {
                Step::Event(ev, used) => {
                    self.buf.drain(..used.max(1));
                    out.push(ev);
                }
                Step::Skip(used) => {
                    self.buf.drain(..used.max(1));
                }
                Step::Incomplete => {
                    // Can't happen with force=true, but guard against a spin.
                    self.buf.clear();
                    break;
                }
            }
        }
        out
    }

    /// True when no partial sequence is buffered.
    pub fn is_idle(&self) -> bool {
        self.buf.is_empty()
    }
}

fn parse_one(buf: &[u8], force: bool) -> Step {
    let Some(&first) = buf.first() else {
        return Step::Incomplete;
    };
    match first {
        0x1b => {
            // Bracketed paste is a CSI sequence but carries an arbitrary payload,
            // so it must be detected before generic CSI handling.
            if let Some(step) = try_paste(buf, force) {
                return step;
            }
            parse_escape(buf, force)
        }
        b'\r' | b'\n' => Step::Event(Event::Key(KeyEvent::new(KeyCode::Enter)), 1),
        b'\t' => Step::Event(Event::Key(KeyEvent::new(KeyCode::Tab)), 1),
        0x7f | 0x08 => Step::Event(Event::Key(KeyEvent::new(KeyCode::Backspace)), 1),
        0x00 => Step::Event(
            Event::Key(KeyEvent::with(KeyCode::Char(' '), Modifiers::CTRL)),
            1,
        ),
        0x01..=0x1a => {
            // Ctrl-a .. Ctrl-z (0x08/0x09/0x0d handled above).
            let c = (first - 1 + b'a') as char;
            Step::Event(Event::Key(KeyEvent::with(KeyCode::Char(c), Modifiers::CTRL)), 1)
        }
        0x1c..=0x1f => {
            let c = (first + 0x40) as char; // Ctrl-\ ] ^ _
            Step::Event(Event::Key(KeyEvent::with(KeyCode::Char(c), Modifiers::CTRL)), 1)
        }
        _ => parse_utf8(buf, Modifiers::NONE, 0),
    }
}

/// Decode one UTF-8 scalar starting at `buf[start]`, tagging it with `mods`.
fn parse_utf8(buf: &[u8], mods: Modifiers, start: usize) -> Step {
    let b = buf[start];
    let len = utf8_len(b);
    if len == 0 {
        // Invalid lead byte; drop it.
        return Step::Skip(start + 1);
    }
    if buf.len() < start + len {
        return Step::Incomplete;
    }
    match core::str::from_utf8(&buf[start..start + len]) {
        Ok(s) => match s.chars().next() {
            Some(c) => Step::Event(Event::Key(KeyEvent::with(KeyCode::Char(c), mods)), start + len),
            None => Step::Skip(start + len),
        },
        Err(_) => Step::Skip(start + 1),
    }
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        0
    }
}

fn parse_escape(buf: &[u8], force: bool) -> Step {
    // Just an ESC so far.
    if buf.len() == 1 {
        if force {
            return Step::Event(Event::Key(KeyEvent::new(KeyCode::Esc)), 1);
        }
        return Step::Incomplete;
    }
    match buf[1] {
        b'[' => parse_csi(buf, force),
        b'O' => parse_ss3(buf, force),
        0x1b => {
            // ESC ESC: treat the leading ESC as an Alt-modified Escape only if
            // forced; otherwise wait (could be ESC then a sequence).
            if force {
                Step::Event(Event::Key(KeyEvent::with(KeyCode::Esc, Modifiers::ALT)), 1)
            } else {
                Step::Incomplete
            }
        }
        _ => {
            // ESC + key  =>  Alt + key. Re-run the single-byte / utf8 logic on
            // the following byte and OR in ALT.
            alt_prefixed(buf)
        }
    }
}

/// Parse `ESC <key>` as Alt + key.
fn alt_prefixed(buf: &[u8]) -> Step {
    let b = buf[1];
    let tag = |code: KeyCode, used: usize| {
        Step::Event(Event::Key(KeyEvent::with(code, Modifiers::ALT)), used)
    };
    match b {
        b'\r' | b'\n' => tag(KeyCode::Enter, 2),
        b'\t' => tag(KeyCode::Tab, 2),
        0x7f | 0x08 => tag(KeyCode::Backspace, 2),
        0x01..=0x1a => tag(KeyCode::Char((b - 1 + b'a') as char), 2),
        _ => match parse_utf8(buf, Modifiers::ALT, 1) {
            Step::Event(ev, used) => Step::Event(ev, used),
            other => other,
        },
    }
}

/// Parse a CSI sequence: `ESC [ [<] params final`.
fn parse_csi(buf: &[u8], force: bool) -> Step {
    // Mouse variants first.
    if buf.get(2) == Some(&b'<') {
        return parse_sgr_mouse(buf, force);
    }
    if buf.get(2) == Some(&b'M') {
        return parse_x10_mouse(buf);
    }
    if buf.get(2) == Some(&b'I') {
        return Step::Event(Event::FocusGained, 3);
    }
    if buf.get(2) == Some(&b'O') {
        return Step::Event(Event::FocusLost, 3);
    }

    // Collect the numeric parameter block, then a final byte in 0x40..=0x7e.
    let mut i = 2;
    while i < buf.len() && (buf[i].is_ascii_digit() || buf[i] == b';') {
        i += 1;
    }
    if i >= buf.len() {
        return if force { Step::Skip(buf.len()) } else { Step::Incomplete };
    }
    let final_byte = buf[i];
    let params = parse_params(&buf[2..i]);
    let used = i + 1;

    // The second parameter, if present, carries the modifier mask (base 1).
    let modifier = params.get(1).copied().filter(|&v| v > 0).map(|v| Modifiers::from_xterm_mask((v - 1) as u8)).unwrap_or(Modifiers::NONE);

    let code = match final_byte {
        b'A' => Some(KeyCode::Up),
        b'B' => Some(KeyCode::Down),
        b'C' => Some(KeyCode::Right),
        b'D' => Some(KeyCode::Left),
        b'H' => Some(KeyCode::Home),
        b'F' => Some(KeyCode::End),
        b'Z' => return Step::Event(Event::Key(KeyEvent::with(KeyCode::BackTab, modifier)), used),
        b'P' => Some(KeyCode::F(1)),
        b'Q' => Some(KeyCode::F(2)),
        b'R' => Some(KeyCode::F(3)),
        b'S' => Some(KeyCode::F(4)),
        b'~' => Some(tilde_code(params.first().copied().unwrap_or(0))),
        _ => None,
    };

    match code {
        Some(KeyCode::Null) => Step::Skip(used), // sentinel used by paste/unknown
        Some(c) => Step::Event(Event::Key(KeyEvent::with(c, modifier)), used),
        None => Step::Skip(used),
    }
}

/// Map the numeric parameter of a `CSI n ~` sequence to a key. Unknown values
/// (including the paste markers, which are handled before we get here) map to
/// [`KeyCode::Null`], which the caller treats as "skip".
fn tilde_code(n: u32) -> KeyCode {
    match n {
        1 | 7 => KeyCode::Home,
        2 => KeyCode::Insert,
        3 => KeyCode::Delete,
        4 | 8 => KeyCode::End,
        5 => KeyCode::PageUp,
        6 => KeyCode::PageDown,
        11 => KeyCode::F(1),
        12 => KeyCode::F(2),
        13 => KeyCode::F(3),
        14 => KeyCode::F(4),
        15 => KeyCode::F(5),
        17 => KeyCode::F(6),
        18 => KeyCode::F(7),
        19 => KeyCode::F(8),
        20 => KeyCode::F(9),
        21 => KeyCode::F(10),
        23 => KeyCode::F(11),
        24 => KeyCode::F(12),
        _ => KeyCode::Null,
    }
}

/// Parse SGR mouse: `ESC [ < b ; x ; y (M|m)`.
fn parse_sgr_mouse(buf: &[u8], force: bool) -> Step {
    let mut i = 3;
    while i < buf.len() && (buf[i].is_ascii_digit() || buf[i] == b';') {
        i += 1;
    }
    if i >= buf.len() {
        return if force { Step::Skip(buf.len()) } else { Step::Incomplete };
    }
    let terminator = buf[i];
    if terminator != b'M' && terminator != b'm' {
        return Step::Skip(i + 1);
    }
    let params = parse_params(&buf[3..i]);
    let used = i + 1;
    let (Some(&cb), Some(&cx), Some(&cy)) = (params.first(), params.get(1), params.get(2)) else {
        return Step::Skip(used);
    };
    let pressed = terminator == b'M';
    let event = decode_mouse(cb, cx.saturating_sub(1) as u16, cy.saturating_sub(1) as u16, pressed);
    match event {
        Some(ev) => Step::Event(Event::Mouse(ev), used),
        None => Step::Skip(used),
    }
}

/// Parse legacy X10 mouse: `ESC [ M b x y` (three bytes, each +32).
fn parse_x10_mouse(buf: &[u8]) -> Step {
    if buf.len() < 6 {
        return Step::Incomplete;
    }
    let cb = buf[3].wrapping_sub(32) as u32;
    let cx = (buf[4] as u16).saturating_sub(33);
    let cy = (buf[5] as u16).saturating_sub(33);
    // In X10, button 3 (low bits == 3) means release.
    let pressed = (cb & 0b11) != 0b11;
    match decode_mouse(cb, cx, cy, pressed) {
        Some(ev) => Step::Event(Event::Mouse(ev), 6),
        None => Step::Skip(6),
    }
}

/// Turn a mouse button/flag code plus coordinates into a [`MouseEvent`].
fn decode_mouse(cb: u32, column: u16, row: u16, pressed: bool) -> Option<MouseEvent> {
    let mut modifiers = Modifiers::NONE;
    if cb & 0x04 != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if cb & 0x08 != 0 {
        modifiers |= Modifiers::ALT;
    }
    if cb & 0x10 != 0 {
        modifiers |= Modifiers::CTRL;
    }

    let motion = cb & 0x20 != 0;
    let wheel = cb & 0x40 != 0;
    let low = cb & 0x03;

    let kind = if wheel {
        match low {
            0 => MouseKind::ScrollUp,
            1 => MouseKind::ScrollDown,
            2 => MouseKind::ScrollLeft,
            3 => MouseKind::ScrollRight,
            _ => return None,
        }
    } else {
        let button = match low {
            0 => Some(MouseButton::Left),
            1 => Some(MouseButton::Middle),
            2 => Some(MouseButton::Right),
            _ => None, // 3 = release-of-unknown in SGR uses the terminator instead
        };
        match (motion, pressed, button) {
            (true, _, Some(b)) => MouseKind::Drag(b),
            (true, _, None) => MouseKind::Moved,
            (false, true, Some(b)) => MouseKind::Down(b),
            (false, false, Some(b)) => MouseKind::Up(b),
            // No button bits but a release: treat as Up(Left) as a safe default.
            (false, false, None) => MouseKind::Up(MouseButton::Left),
            (false, true, None) => return None,
        }
    };

    Some(MouseEvent { kind, column, row, modifiers })
}

/// Parse an SS3 sequence: `ESC O <byte>`.
fn parse_ss3(buf: &[u8], force: bool) -> Step {
    if buf.len() < 3 {
        return if force { Step::Skip(buf.len()) } else { Step::Incomplete };
    }
    let code = match buf[2] {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        _ => return Step::Skip(3),
    };
    Step::Event(Event::Key(KeyEvent::new(code)), 3)
}

/// Split a `;`-separated decimal parameter list. Empty fields become 0.
fn parse_params(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cur: u32 = 0;
    let mut has_digit = false;
    for &b in bytes {
        if b == b';' {
            out.push(cur);
            cur = 0;
            has_digit = false;
        } else if b.is_ascii_digit() {
            cur = cur.saturating_mul(10).saturating_add((b - b'0') as u32);
            has_digit = true;
        }
    }
    // Always push the final field (even if the list was empty, callers index
    // defensively). Only skip the trailing push when there were no bytes at all.
    if has_digit || !bytes.is_empty() {
        out.push(cur);
    }
    out
}

// ----- bracketed paste ---------------------------------------------------

/// Detect and extract a bracketed-paste block at the front of `buf`.
///
/// Returns the decoded [`Event::Paste`] and bytes consumed, or `None` if this is
/// not a paste-start. Called from [`Parser::feed`] before general CSI handling.
fn try_paste(buf: &[u8], force: bool) -> Option<Step> {
    const START: &[u8] = b"\x1b[200~";
    const END: &[u8] = b"\x1b[201~";
    if !buf.starts_with(START) {
        return None;
    }
    let body = &buf[START.len()..];
    if let Some(pos) = find_subslice(body, END) {
        let text = String::from_utf8_lossy(&body[..pos]).into_owned();
        let used = START.len() + pos + END.len();
        Some(Step::Event(Event::Paste(text), used))
    } else if force {
        // Unterminated paste at flush: take whatever we have.
        let text = String::from_utf8_lossy(body).into_owned();
        Some(Step::Event(Event::Paste(text), buf.len()))
    } else {
        Some(Step::Incomplete)
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn events(bytes: &[u8]) -> Vec<Event> {
        let mut p = Parser::new();
        p.feed(bytes)
    }

    #[test]
    fn plain_ascii() {
        assert_eq!(
            events(b"hi"),
            vec![
                Event::Key(KeyEvent::new(KeyCode::Char('h'))),
                Event::Key(KeyEvent::new(KeyCode::Char('i'))),
            ]
        );
    }

    #[test]
    fn ctrl_and_special() {
        assert_eq!(events(b"\x01"), vec![Event::Key(KeyEvent::with(KeyCode::Char('a'), Modifiers::CTRL))]);
        assert_eq!(events(b"\r"), vec![Event::Key(KeyEvent::new(KeyCode::Enter))]);
        assert_eq!(events(b"\x7f"), vec![Event::Key(KeyEvent::new(KeyCode::Backspace))]);
    }

    #[test]
    fn arrows_and_modifiers() {
        assert_eq!(events(b"\x1b[A"), vec![Event::Key(KeyEvent::new(KeyCode::Up))]);
        assert_eq!(
            events(b"\x1b[1;5A"),
            vec![Event::Key(KeyEvent::with(KeyCode::Up, Modifiers::CTRL))]
        );
        assert_eq!(
            events(b"\x1b[3;2~"),
            vec![Event::Key(KeyEvent::with(KeyCode::Delete, Modifiers::SHIFT))]
        );
    }

    #[test]
    fn alt_prefixed_key() {
        assert_eq!(
            events(b"\x1bx"),
            vec![Event::Key(KeyEvent::with(KeyCode::Char('x'), Modifiers::ALT))]
        );
    }

    #[test]
    fn lone_escape_needs_flush() {
        let mut p = Parser::new();
        assert!(p.feed(b"\x1b").is_empty());
        assert_eq!(p.flush(), vec![Event::Key(KeyEvent::new(KeyCode::Esc))]);
    }

    #[test]
    fn sgr_mouse_click() {
        let evs = events(b"\x1b[<0;10;5M");
        assert_eq!(
            evs,
            vec![Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                column: 9,
                row: 4,
                modifiers: Modifiers::NONE,
            })]
        );
    }

    #[test]
    fn sgr_mouse_release_and_wheel() {
        let up = events(b"\x1b[<0;1;1m");
        assert!(matches!(up[0], Event::Mouse(MouseEvent { kind: MouseKind::Up(MouseButton::Left), .. })));
        let wheel = events(b"\x1b[<64;1;1M");
        assert!(matches!(wheel[0], Event::Mouse(MouseEvent { kind: MouseKind::ScrollUp, .. })));
    }

    #[test]
    fn split_sequence_across_feeds() {
        let mut p = Parser::new();
        assert!(p.feed(b"\x1b[").is_empty());
        assert!(p.feed(b"1;5").is_empty());
        assert_eq!(p.feed(b"C"), vec![Event::Key(KeyEvent::with(KeyCode::Right, Modifiers::CTRL))]);
    }

    #[test]
    fn utf8_multibyte() {
        assert_eq!(events("é".as_bytes()), vec![Event::Key(KeyEvent::new(KeyCode::Char('é')))]);
    }

    #[test]
    fn paste_block() {
        assert_eq!(
            events(b"\x1b[200~hello\x1b[201~"),
            vec![Event::Paste(String::from("hello"))]
        );
    }
}
