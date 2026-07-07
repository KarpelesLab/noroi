//! C ABI for noroi (enabled by the `capi` feature).
//!
//! This exposes an imperative, handle-based interface mirroring the Rust
//! [`Terminal`](crate::terminal::Terminal): open a terminal, then for each frame
//! call [`noroi_begin`], issue draw calls, and call [`noroi_end`]; poll input
//! with [`noroi_poll_event`]. The matching header is `include/noroi.h`.
//!
//! Colors are opaque `uint32_t` values built with the `noroi_color_*`
//! constructors; attributes are an OR of the `NOROI_ATTR_*` bits. All strings
//! are NUL-terminated UTF-8. Functions that take a terminal pointer are safe to
//! call with `NULL` (they become no-ops / return an error code).
//!
//! # Safety
//! Every function here is `extern "C"` and dereferences caller-provided
//! pointers. Callers must pass pointers obtained from this API (for terminals)
//! and valid, NUL-terminated strings, upholding the usual FFI contract.

#![allow(clippy::missing_safety_doc)]

use crate::event::{Event, KeyCode, MouseButton, MouseKind};
use crate::geom::{Point, Rect};
use crate::style::{Attributes, Color, Style};
use crate::terminal::Terminal;
use crate::widget::{Block, BorderType, Gauge, Line, Widget};
use std::ffi::{CStr, CString, c_char, c_int};
use std::time::Duration;

/// Opaque terminal handle returned by [`noroi_open`].
pub struct NoroiTerminal {
    term: Terminal,
    last_paste: CString,
}

// ----- color / attribute encoding ---------------------------------------
//
// A color is a u32 with a 2-bit tag in the high byte:
//   tag 0 => indexed (low byte is the 0..255 palette index / named color)
//   tag 1 => 24-bit RGB in the low 24 bits
//   tag 2 => the terminal default color (SGR 39/49)
//   tag 3 => "none": leave the cell's existing color (transparent)

const TAG_SHIFT: u32 = 24;

/// An indexed / named color (0–255).
#[unsafe(no_mangle)]
pub extern "C" fn noroi_color_indexed(index: u8) -> u32 {
    index as u32
}

/// A 24-bit true color.
#[unsafe(no_mangle)]
pub extern "C" fn noroi_color_rgb(r: u8, g: u8, b: u8) -> u32 {
    (1 << TAG_SHIFT) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// The terminal's configured default color.
#[unsafe(no_mangle)]
pub extern "C" fn noroi_color_default() -> u32 {
    2 << TAG_SHIFT
}

/// A transparent color: keep whatever color the cell already had.
#[unsafe(no_mangle)]
pub extern "C" fn noroi_color_none() -> u32 {
    3 << TAG_SHIFT
}

fn decode_color(v: u32) -> Option<Color> {
    match v >> TAG_SHIFT {
        1 => Some(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)),
        2 => Some(Color::Reset),
        3 => None,
        _ => Some(Color::Indexed((v & 0xff) as u8)),
    }
}

fn make_style(fg: u32, bg: u32, attrs: u16) -> Style {
    Style {
        fg: decode_color(fg),
        bg: decode_color(bg),
        attributes: Attributes::from_bits(attrs),
    }
}

fn border_type(kind: c_int) -> BorderType {
    match kind {
        1 => BorderType::Rounded,
        2 => BorderType::Double,
        3 => BorderType::Thick,
        _ => BorderType::Plain,
    }
}

// ----- helpers -----------------------------------------------------------

/// Turn a raw handle into a reference, or `None` for NULL.
///
/// # Safety
/// `ptr` must be NULL or a valid pointer from [`noroi_open`].
unsafe fn handle<'a>(ptr: *mut NoroiTerminal) -> Option<&'a mut NoroiTerminal> {
    unsafe { ptr.as_mut() }
}

/// Borrow a C string as `&str` (lossy), or `""` for NULL / invalid UTF-8.
///
/// # Safety
/// `s` must be NULL or a valid NUL-terminated string.
unsafe fn cstr(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
}

// ----- lifecycle ---------------------------------------------------------

/// Open the controlling terminal (raw mode, alternate screen, mouse, paste and
/// focus reporting). Returns NULL on failure.
#[unsafe(no_mangle)]
pub extern "C" fn noroi_open() -> *mut NoroiTerminal {
    match Terminal::open() {
        Ok(term) => Box::into_raw(Box::new(NoroiTerminal {
            term,
            last_paste: CString::default(),
        })),
        Err(_) => core::ptr::null_mut(),
    }
}

/// Close a terminal, restoring the previous mode. Safe to call with NULL.
///
/// # Safety
/// `t` must be NULL or a handle from [`noroi_open`], not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_close(t: *mut NoroiTerminal) {
    if !t.is_null() {
        drop(unsafe { Box::from_raw(t) });
    }
}

/// Write the terminal size (in cells) into `*cols` / `*rows`. Returns 0 on
/// success, -1 on error.
///
/// # Safety
/// `cols` and `rows` must be valid, writable pointers (or NULL to skip).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_size(
    t: *mut NoroiTerminal,
    cols: *mut u16,
    rows: *mut u16,
) -> c_int {
    let Some(h) = (unsafe { handle(t) }) else {
        return -1;
    };
    let size = h.term.size();
    unsafe {
        if !cols.is_null() {
            *cols = size.width;
        }
        if !rows.is_null() {
            *rows = size.height;
        }
    }
    0
}

// ----- frame lifecycle ---------------------------------------------------

/// Begin a frame: sync the size and clear the working buffer. Returns 0/-1.
///
/// # Safety
/// `t` must be NULL or a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_begin(t: *mut NoroiTerminal) -> c_int {
    let Some(h) = (unsafe { handle(t) }) else {
        return -1;
    };
    match h.term.start_frame() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// End a frame: render the diff and flush. Returns 0/-1.
///
/// # Safety
/// `t` must be NULL or a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_end(t: *mut NoroiTerminal) -> c_int {
    let Some(h) = (unsafe { handle(t) }) else {
        return -1;
    };
    match h.term.finish_frame() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Position the hardware cursor for this frame (shown at end of frame).
///
/// # Safety
/// `t` must be NULL or a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_set_cursor(t: *mut NoroiTerminal, x: u16, y: u16) {
    if let Some(h) = unsafe { handle(t) } {
        h.term.set_frame_cursor(Point::new(x, y));
    }
}

// ----- drawing -----------------------------------------------------------

/// Draw UTF-8 `text` at `(x, y)`, clipped to `max_width` columns.
///
/// # Safety
/// `t` must be a valid handle and `text` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_text(
    t: *mut NoroiTerminal,
    x: u16,
    y: u16,
    text: *const c_char,
    fg: u32,
    bg: u32,
    attrs: u16,
    max_width: u16,
) {
    let Some(h) = (unsafe { handle(t) }) else {
        return;
    };
    let s = unsafe { cstr(text) };
    let style = make_style(fg, bg, attrs);
    h.term
        .frame_buffer_mut()
        .set_str(x, y, &s, style, max_width);
}

/// Fill a rectangle with `ch` (the first character of a UTF-8 string).
///
/// # Safety
/// `t` must be a valid handle and `ch` a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_fill(
    t: *mut NoroiTerminal,
    x: u16,
    y: u16,
    w: u16,
    h_: u16,
    ch: *const c_char,
    fg: u32,
    bg: u32,
    attrs: u16,
) {
    let Some(h) = (unsafe { handle(t) }) else {
        return;
    };
    let s = unsafe { cstr(ch) };
    let c = s.chars().next().unwrap_or(' ');
    let style = make_style(fg, bg, attrs);
    let buf = h.term.frame_buffer_mut();
    for yy in y..y.saturating_add(h_) {
        for xx in x..x.saturating_add(w) {
            buf.set_char(xx, yy, c, style);
        }
    }
}

/// Draw a bordered box (optionally titled) over `(x, y, w, h)`.
/// `border` selects the line style: 0 plain, 1 rounded, 2 double, 3 thick.
///
/// # Safety
/// `t` must be a valid handle; `title` may be NULL or a valid string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_box(
    t: *mut NoroiTerminal,
    x: u16,
    y: u16,
    w: u16,
    h_: u16,
    border: c_int,
    title: *const c_char,
    fg: u32,
    bg: u32,
    attrs: u16,
) {
    let Some(h) = (unsafe { handle(t) }) else {
        return;
    };
    let border_style = make_style(fg, noroi_color_none(), attrs);
    let mut block = Block::bordered()
        .border_type(border_type(border))
        .border_style(border_style);
    if let Some(bgc) = decode_color(bg) {
        block = block.style(Style::new().bg(bgc));
    }
    if !title.is_null() {
        let title = unsafe { cstr(title) };
        block = block.title(Line::raw(title));
    }
    let area = Rect::new(x, y, w, h_);
    block.render(area, h.term.frame_buffer_mut());
}

/// Draw a progress gauge filled to `ratio` (0.0–1.0) over `(x, y, w, h)`.
///
/// # Safety
/// `t` must be a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_gauge(
    t: *mut NoroiTerminal,
    x: u16,
    y: u16,
    w: u16,
    h_: u16,
    ratio: f32,
    filled_fg: u32,
    filled_bg: u32,
    unfilled_fg: u32,
    unfilled_bg: u32,
) {
    let Some(h) = (unsafe { handle(t) }) else {
        return;
    };
    let gauge = Gauge::new()
        .ratio(ratio)
        .filled_style(make_style(filled_fg, filled_bg, 0))
        .unfilled_style(make_style(unfilled_fg, unfilled_bg, 0));
    gauge.render(Rect::new(x, y, w, h_), h.term.frame_buffer_mut());
}

// ----- input -------------------------------------------------------------

/// A decoded input event, filled by [`noroi_poll_event`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NoroiEvent {
    /// 0 none, 1 key, 2 mouse, 3 resize, 4 paste, 5 focus.
    pub kind: c_int,
    /// For a key: a special-key code (see `NOROI_KEY_*`), or 0 for a character.
    pub key: u32,
    /// For a character key: the Unicode scalar; otherwise 0.
    pub ch: u32,
    /// Modifier bitset (`NOROI_MOD_*`).
    pub modifiers: u8,
    /// For a mouse event: 0 down, 1 up, 2 drag, 3 moved, 4–7 scroll u/d/l/r.
    pub mouse_kind: c_int,
    /// For a mouse button event: 0 left, 1 middle, 2 right, -1 none.
    pub mouse_button: c_int,
    /// Mouse column, resize columns, or 1/0 for focus gained/lost.
    pub x: u16,
    /// Mouse row or resize rows.
    pub y: u16,
}

impl Default for NoroiEvent {
    fn default() -> Self {
        NoroiEvent {
            kind: 0,
            key: 0,
            ch: 0,
            modifiers: 0,
            mouse_kind: -1,
            mouse_button: -1,
            x: 0,
            y: 0,
        }
    }
}

fn key_code_value(code: KeyCode) -> (u32, u32) {
    // Returns (special_key, character_scalar).
    match code {
        KeyCode::Char(c) => (0, c as u32),
        KeyCode::Enter => (1, 0),
        KeyCode::Tab => (2, 0),
        KeyCode::Backspace => (3, 0),
        KeyCode::Esc => (4, 0),
        KeyCode::Left => (5, 0),
        KeyCode::Right => (6, 0),
        KeyCode::Up => (7, 0),
        KeyCode::Down => (8, 0),
        KeyCode::Home => (9, 0),
        KeyCode::End => (10, 0),
        KeyCode::PageUp => (11, 0),
        KeyCode::PageDown => (12, 0),
        KeyCode::Insert => (13, 0),
        KeyCode::Delete => (14, 0),
        KeyCode::BackTab => (15, 0),
        KeyCode::F(n) => (100 + n as u32, 0),
        KeyCode::Null => (999, 0),
    }
}

fn mouse_values(kind: MouseKind) -> (c_int, c_int) {
    let button = |b: MouseButton| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    match kind {
        MouseKind::Down(b) => (0, button(b)),
        MouseKind::Up(b) => (1, button(b)),
        MouseKind::Drag(b) => (2, button(b)),
        MouseKind::Moved => (3, -1),
        MouseKind::ScrollUp => (4, -1),
        MouseKind::ScrollDown => (5, -1),
        MouseKind::ScrollLeft => (6, -1),
        MouseKind::ScrollRight => (7, -1),
    }
}

/// Poll for the next input event, waiting up to `timeout_ms` (negative = block).
///
/// Writes the event into `*out` and returns 1 if one arrived, 0 on timeout, and
/// -1 on error. For a paste event, retrieve the text with [`noroi_paste_text`].
///
/// # Safety
/// `t` must be a valid handle and `out` a valid, writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_poll_event(
    t: *mut NoroiTerminal,
    timeout_ms: c_int,
    out: *mut NoroiEvent,
) -> c_int {
    let Some(h) = (unsafe { handle(t) }) else {
        return -1;
    };
    if out.is_null() {
        return -1;
    }
    let timeout = if timeout_ms < 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms as u64))
    };
    let event = match h.term.events().poll(timeout) {
        Ok(Some(ev)) => ev,
        Ok(None) => return 0,
        Err(_) => return -1,
    };

    let mut ev = NoroiEvent::default();
    match event {
        Event::Key(k) => {
            ev.kind = 1;
            let (key, ch) = key_code_value(k.code);
            ev.key = key;
            ev.ch = ch;
            ev.modifiers = k.modifiers.bits();
        }
        Event::Mouse(m) => {
            ev.kind = 2;
            let (mk, btn) = mouse_values(m.kind);
            ev.mouse_kind = mk;
            ev.mouse_button = btn;
            ev.modifiers = m.modifiers.bits();
            ev.x = m.column;
            ev.y = m.row;
        }
        Event::Resize(cols, rows) => {
            ev.kind = 3;
            ev.x = cols;
            ev.y = rows;
        }
        Event::Paste(text) => {
            ev.kind = 4;
            h.last_paste = CString::new(text.replace('\0', "")).unwrap_or_default();
        }
        Event::FocusGained => {
            ev.kind = 5;
            ev.x = 1;
        }
        Event::FocusLost => {
            ev.kind = 5;
            ev.x = 0;
        }
    }
    unsafe {
        *out = ev;
    }
    1
}

/// Return the text of the most recent paste event as a NUL-terminated string.
/// The pointer is valid until the next [`noroi_poll_event`] call.
///
/// # Safety
/// `t` must be a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn noroi_paste_text(t: *mut NoroiTerminal) -> *const c_char {
    match unsafe { handle(t) } {
        Some(h) => h.last_paste.as_ptr(),
        None => core::ptr::null(),
    }
}
