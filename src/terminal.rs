//! The high-level terminal driver.
//!
//! [`Terminal`] ties everything together: it owns a [`Backend`], a background
//! [`EventStream`], and two [`Buffer`]s (front = on screen, back = being
//! painted). Each [`draw`](Terminal::draw) call hands your closure a [`Frame`]
//! to paint widgets into, then diffs the new frame against what is on screen and
//! sends only the changes to the backend — the classic double-buffered,
//! flicker-free redraw.
//!
//! Opening a terminal enters raw mode and (by default) the alternate screen with
//! mouse, paste and focus reporting on. Dropping it restores everything.

use crate::backend::{Backend, Features, UnixBackend};
use crate::buffer::Buffer;
use crate::events::EventStream;
use crate::geom::{Point, Rect, Size};
use crate::widget::{StatefulWidget, Widget};
use core::mem;
use std::io;

/// A driver over a [`Backend`] providing double-buffered frame rendering and an
/// input event stream.
#[derive(Debug)]
pub struct Terminal<B: Backend = UnixBackend> {
    backend: B,
    /// What is currently on screen.
    front: Buffer,
    /// Scratch buffer painted each frame.
    back: Buffer,
    size: Size,
    events: EventStream,
    /// Where to place the hardware cursor after the next flush, if anywhere.
    cursor: Option<Point>,
}

impl Terminal<UnixBackend> {
    /// Open the controlling terminal with default [`Features`] (alternate
    /// screen, mouse, paste and focus reporting all enabled).
    pub fn open() -> io::Result<Self> {
        Terminal::with_features(Features::default())
    }

    /// Open with a specific feature set.
    pub fn with_features(features: Features) -> io::Result<Self> {
        let backend = UnixBackend::open(features)?;
        let events = EventStream::spawn(backend.reader()?);
        Terminal::from_parts(backend, events)
    }
}

impl<B: Backend> Terminal<B> {
    /// Build a terminal from an already-constructed backend and event stream.
    ///
    /// Useful for tests or alternative backends.
    pub fn from_parts(backend: B, events: EventStream) -> io::Result<Self> {
        let size = backend.size()?;
        let area = Rect::from_size(size);
        Ok(Terminal {
            backend,
            front: Buffer::empty(area),
            back: Buffer::empty(area),
            size,
            events,
            cursor: None,
        })
    }

    /// The current terminal size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The full screen rectangle.
    pub fn area(&self) -> Rect {
        Rect::from_size(self.size)
    }

    /// Access the input [`EventStream`].
    pub fn events(&self) -> &EventStream {
        &self.events
    }

    /// The underlying backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Mutable access to the backend (e.g. to simulate a resize in tests).
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// The buffer currently on screen (the last fully drawn frame).
    ///
    /// Handy for snapshot tests and for inspecting what was rendered.
    pub fn current_buffer(&self) -> &Buffer {
        &self.front
    }

    /// Re-query the terminal size and, if it changed, resize the buffers and
    /// request a full repaint on the next frame.
    ///
    /// Called automatically by [`draw`](Self::draw); exposed for callers that
    /// want to react to a [`Resize`](crate::event::Event::Resize) event eagerly.
    pub fn sync_size(&mut self) -> io::Result<bool> {
        let new = self.backend.size()?;
        if new != self.size {
            self.size = new;
            let area = Rect::from_size(new);
            self.back.resize(area);
            // Force a full repaint: make `front` differ everywhere by resizing
            // it to an empty area, and clear the physical screen.
            self.front.resize(Rect::ZERO);
            self.backend.clear()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Discard the terminal's knowledge of the screen so the next
    /// [`draw`](Self::draw) repaints every cell.
    pub fn force_redraw(&mut self) -> io::Result<()> {
        self.front.resize(Rect::ZERO);
        self.backend.clear()
    }

    /// Paint a frame.
    ///
    /// The closure receives a [`Frame`] whose buffer is blank and sized to the
    /// screen. After it returns, only cells that differ from what is on screen
    /// are written, and the hardware cursor is positioned (or hidden).
    pub fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        self.start_frame()?;
        let mut frame = Frame {
            buffer: &mut self.back,
            cursor: None,
        };
        render(&mut frame);
        self.cursor = frame.cursor;
        self.finish_frame()
    }

    /// Begin a manually-driven frame: sync the size and clear the back buffer.
    ///
    /// This is the imperative counterpart to [`draw`](Self::draw), used by the
    /// C bindings and by callers who cannot express rendering as a closure.
    /// Paint via [`frame_buffer_mut`](Self::frame_buffer_mut), optionally call
    /// [`set_frame_cursor`](Self::set_frame_cursor), then
    /// [`finish_frame`](Self::finish_frame).
    pub fn start_frame(&mut self) -> io::Result<()> {
        self.sync_size()?;
        self.back.reset();
        self.cursor = None;
        Ok(())
    }

    /// The buffer being painted for the current manual frame.
    pub fn frame_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.back
    }

    /// Request the hardware cursor position for the current manual frame.
    pub fn set_frame_cursor(&mut self, position: Point) {
        self.cursor = Some(position);
    }

    /// Finish a manual frame: diff against the screen, write changes, position
    /// the cursor and flush.
    pub fn finish_frame(&mut self) -> io::Result<()> {
        let diff = self.back.diff(&self.front);
        self.backend.draw(&diff)?;
        match self.cursor {
            Some(p) => {
                self.backend.set_cursor(p.x, p.y)?;
                self.backend.show_cursor()?;
            }
            None => self.backend.hide_cursor()?,
        }
        self.backend.flush()?;
        mem::swap(&mut self.front, &mut self.back);
        Ok(())
    }
}

/// The canvas handed to a [`Terminal::draw`] closure.
///
/// A frame exposes the screen-sized [`Buffer`] to paint into, plus a place to
/// record where the hardware cursor should end up (for text inputs and the
/// like). Widgets are rendered with [`render_widget`](Frame::render_widget).
#[derive(Debug)]
pub struct Frame<'a> {
    buffer: &'a mut Buffer,
    cursor: Option<Point>,
}

impl<'a> Frame<'a> {
    /// The area available this frame (the whole screen).
    pub fn area(&self) -> Rect {
        self.buffer.area()
    }

    /// Mutable access to the frame's buffer.
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        self.buffer
    }

    /// Render a [`Widget`] into `area`.
    pub fn render_widget<W: Widget>(&mut self, widget: &W, area: Rect) {
        widget.render(area, self.buffer);
    }

    /// Render a [`StatefulWidget`] into `area`, updating `state`.
    pub fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: &W,
        area: Rect,
        state: &mut W::State,
    ) {
        widget.render_stateful(area, self.buffer, state);
    }

    /// Request that the hardware cursor be shown at `position` after this frame.
    pub fn set_cursor(&mut self, position: Point) {
        self.cursor = Some(position);
    }
}
