//! The rendering backend: turns buffer diffs into terminal writes.
//!
//! [`Backend`] is the narrow interface the [`Terminal`](crate::terminal::Terminal)
//! driver needs. [`UnixBackend`] implements it against a unix TTY: it owns the
//! raw-mode guard and the alternate-screen / mouse / paste / focus modes, and
//! translates a [`Buffer`](crate::buffer::Buffer) diff into the minimal run of
//! cursor moves and SGR sequences.

use crate::ansi;
use crate::buffer::{Cell, char_width};
use crate::geom::{Point, Size};
use crate::style::Style;
use crate::sys::{self, RawModeGuard, Tty};
use alloc::vec::Vec;
use std::io::{self, Write};

/// Optional terminal features to enable on startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// Switch to the alternate screen (restored on exit).
    pub alternate_screen: bool,
    /// Enable mouse reporting.
    pub mouse: bool,
    /// Enable bracketed paste.
    pub bracketed_paste: bool,
    /// Enable focus in/out reporting.
    pub focus: bool,
}

impl Default for Features {
    fn default() -> Self {
        Features {
            alternate_screen: true,
            mouse: true,
            bracketed_paste: true,
            focus: true,
        }
    }
}

/// The interface the terminal driver renders through.
///
/// A backend receives already-computed diffs (lists of changed cells in
/// row-major order) and is responsible only for emitting bytes. This keeps
/// diffing and layout platform-independent and makes alternative backends
/// (a test recorder, a Windows console, an in-memory image) straightforward.
pub trait Backend {
    /// Current terminal size in cells.
    fn size(&self) -> io::Result<Size>;
    /// Clear the whole screen.
    fn clear(&mut self) -> io::Result<()>;
    /// Hide the hardware cursor.
    fn hide_cursor(&mut self) -> io::Result<()>;
    /// Show the hardware cursor.
    fn show_cursor(&mut self) -> io::Result<()>;
    /// Move the hardware cursor to `(x, y)` (zero-based).
    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()>;
    /// Apply a diff: paint each `(position, cell)` with minimal output.
    fn draw(&mut self, diff: &[(Point, &Cell)]) -> io::Result<()>;
    /// Flush buffered output to the device.
    fn flush(&mut self) -> io::Result<()>;
}

/// A unix TTY backend.
#[derive(Debug)]
pub struct UnixBackend {
    tty: Tty,
    caps: ansi::Caps,
    features: Features,
    /// Pending output, flushed as one `write` per frame.
    out: Vec<u8>,
    /// Guard restoring cooked mode on drop; `None` once torn down.
    raw: Option<RawModeGuard>,
}

impl UnixBackend {
    /// Open the controlling terminal, enter raw mode and enable `features`.
    pub fn open(features: Features) -> io::Result<UnixBackend> {
        let tty = sys::open_tty()?;
        let raw = RawModeGuard::new(tty.fd())?;
        let caps = ansi::Caps::detect(
            std::env::var("TERM").ok().as_deref(),
            std::env::var("COLORTERM").ok().as_deref(),
        );
        let mut backend = UnixBackend {
            tty,
            caps,
            features,
            out: Vec::with_capacity(8192),
            raw: Some(raw),
        };
        backend.startup()?;
        Ok(backend)
    }

    /// The detected terminal capabilities.
    pub fn caps(&self) -> ansi::Caps {
        self.caps
    }

    /// A cloned read handle to the same TTY, for the event reader thread.
    pub fn reader(&self) -> io::Result<Tty> {
        self.tty.try_clone()
    }

    fn startup(&mut self) -> io::Result<()> {
        let f = self.features;
        if f.alternate_screen {
            ansi::enter_alternate_screen(&mut self.out);
        }
        if f.mouse {
            ansi::enable_mouse(&mut self.out);
        }
        if f.bracketed_paste {
            ansi::enable_bracketed_paste(&mut self.out);
        }
        if f.focus {
            ansi::enable_focus_reporting(&mut self.out);
        }
        ansi::hide_cursor(&mut self.out);
        ansi::clear_screen(&mut self.out);
        self.flush()
    }

    fn teardown(&mut self) {
        let f = self.features;
        ansi::reset_sgr(&mut self.out);
        ansi::show_cursor(&mut self.out);
        if f.focus {
            ansi::disable_focus_reporting(&mut self.out);
        }
        if f.bracketed_paste {
            ansi::disable_bracketed_paste(&mut self.out);
        }
        if f.mouse {
            ansi::disable_mouse(&mut self.out);
        }
        if f.alternate_screen {
            ansi::leave_alternate_screen(&mut self.out);
        }
        let _ = self.flush();
    }
}

impl Backend for UnixBackend {
    fn size(&self) -> io::Result<Size> {
        let (cols, rows) = sys::window_size(self.tty.fd())?;
        // Fall back to a sane default if the terminal reports nothing.
        let cols = if cols == 0 { 80 } else { cols };
        let rows = if rows == 0 { 24 } else { rows };
        Ok(Size::new(cols, rows))
    }

    fn clear(&mut self) -> io::Result<()> {
        ansi::clear_screen(&mut self.out);
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        ansi::hide_cursor(&mut self.out);
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        ansi::show_cursor(&mut self.out);
        Ok(())
    }

    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        ansi::move_to(&mut self.out, x, y);
        Ok(())
    }

    fn draw(&mut self, diff: &[(Point, &Cell)]) -> io::Result<()> {
        let depth = self.caps.colors;
        let mut last_style = Style::RESET;
        let mut cursor: Option<Point> = None;
        // Begin from a known style state.
        ansi::reset_sgr(&mut self.out);

        for &(pos, cell) in diff {
            // Move only when the cursor is not already where we need it.
            let need_move = match cursor {
                Some(c) => c != pos,
                None => true,
            };
            if need_move {
                ansi::move_to(&mut self.out, pos.x, pos.y);
            }
            if cell.style != last_style {
                ansi::write_style(&mut self.out, &last_style, &cell.style, depth);
                last_style = cell.style;
            }
            self.out.extend_from_slice(cell.symbol().as_bytes());
            let w = char_width(cell.symbol_char()).max(1);
            cursor = Some(Point::new(pos.x.saturating_add(w), pos.y));
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.out.is_empty() {
            return Ok(());
        }
        self.tty.write_all(&self.out)?;
        self.tty.flush()?;
        self.out.clear();
        Ok(())
    }
}

impl Drop for UnixBackend {
    fn drop(&mut self) {
        self.teardown();
        // Dropping `raw` here restores the original terminal mode.
        self.raw.take();
    }
}

/// A headless [`Backend`] that writes nowhere and reports a fixed size.
///
/// It records the total number of cells drawn and the requested cursor
/// position, which — together with [`Terminal::current_buffer`](crate::terminal::Terminal::current_buffer)
/// — makes the whole render pipeline testable without a real terminal.
#[derive(Debug, Clone)]
pub struct TestBackend {
    size: Size,
    /// Count of cells emitted by the most recent [`draw`](Backend::draw).
    pub cells_drawn: usize,
    /// The cursor position requested by [`set_cursor`](Backend::set_cursor).
    pub cursor: Option<(u16, u16)>,
    /// Whether the cursor is currently shown.
    pub cursor_visible: bool,
}

impl TestBackend {
    /// A backend reporting `size`.
    pub fn new(size: Size) -> Self {
        TestBackend {
            size,
            cells_drawn: 0,
            cursor: None,
            cursor_visible: true,
        }
    }

    /// Resize the reported terminal size (simulates a resize).
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }
}

impl Backend for TestBackend {
    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }
    fn clear(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = false;
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = true;
        Ok(())
    }
    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        self.cursor = Some((x, y));
        Ok(())
    }
    fn draw(&mut self, diff: &[(Point, &Cell)]) -> io::Result<()> {
        self.cells_drawn = diff.len();
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
