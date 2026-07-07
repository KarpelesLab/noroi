//! Background terminal-input reading.
//!
//! Reading a TTY blocks, so noroi does it on a dedicated thread. [`EventStream`]
//! spawns that thread, which reads raw bytes, drives an
//! [`input::Parser`](crate::input::Parser), and forwards decoded [`Event`]s over
//! a channel. The main thread consumes them with [`poll`](EventStream::poll)
//! (with an optional timeout) or the blocking [`read`](EventStream::read).
//!
//! Two things are handled here that the pure parser cannot:
//! * **Lone Escape.** When a read times out (100 ms, via the terminal's `VTIME`)
//!   with an unfinished escape buffered, the parser is [`flush`](crate::input::Parser::flush)ed,
//!   turning a solitary `ESC` byte into [`KeyCode::Esc`](crate::event::KeyCode::Esc).
//! * **Resize.** The same idle tick re-queries the window size and emits
//!   [`Event::Resize`] when it changes, so no `SIGWINCH` handler is needed.

use crate::event::Event;
use crate::input::Parser;
use crate::sys::{self, Tty};
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// A stream of input [`Event`]s read from the terminal on a background thread.
#[derive(Debug)]
pub struct EventStream {
    rx: Receiver<Event>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl EventStream {
    /// Spawn the reader thread on `reader` (a clone of the TTY).
    pub fn spawn(reader: Tty) -> EventStream {
        let (tx, rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        let handle = std::thread::Builder::new()
            .name("noroi-input".into())
            .spawn(move || reader_loop(reader, tx, stop))
            .expect("spawn input thread");
        EventStream { rx, shutdown, handle: Some(handle) }
    }

    /// Wait up to `timeout` for the next event.
    ///
    /// Returns `Ok(Some(event))` if one arrived, `Ok(None)` on timeout, and an
    /// error only if the reader thread has terminated. `None` timeout blocks
    /// indefinitely (equivalent to [`read`](Self::read)).
    pub fn poll(&self, timeout: Option<Duration>) -> io::Result<Option<Event>> {
        match timeout {
            None => self.read().map(Some),
            Some(dur) => match self.rx.recv_timeout(dur) {
                Ok(ev) => Ok(Some(ev)),
                Err(RecvTimeoutError::Timeout) => Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    Err(io::Error::new(io::ErrorKind::BrokenPipe, "input thread stopped"))
                }
            },
        }
    }

    /// Block until the next event arrives.
    pub fn read(&self) -> io::Result<Event> {
        self.rx
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "input thread stopped"))
    }

    /// Return the next event if one is already queued, without blocking.
    pub fn try_read(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // The thread checks the flag between reads (≤100 ms), then exits.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn reader_loop(mut reader: Tty, tx: mpsc::Sender<Event>, shutdown: Arc<AtomicBool>) {
    let mut parser = Parser::new();
    let fd = reader.fd();
    let mut last_size = sys::window_size(fd).unwrap_or((80, 24));
    // Announce the initial size so the app can lay out before the first key.
    let _ = tx.send(Event::Resize(last_size.0, last_size.1));

    let mut buf = [0u8; 4096];
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => {
                // Idle tick (VTIME expired): resolve a pending lone ESC…
                for ev in parser.flush() {
                    if tx.send(ev).is_err() {
                        return;
                    }
                }
                // …and check for a resize.
                if let Ok(size) = sys::window_size(fd) {
                    if size != last_size {
                        last_size = size;
                        if tx.send(Event::Resize(size.0, size.1)).is_err() {
                            return;
                        }
                    }
                }
            }
            Ok(n) => {
                for ev in parser.feed(&buf[..n]) {
                    if tx.send(ev).is_err() {
                        return;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
}
