//! # noroi
//!
//! Rich terminal UI for Rust, in the spirit of curses/ncurses, with **zero
//! external crate dependencies** — it uses only `core`, `alloc` and (for the
//! OS-facing layer) `std`.
//!
//! The crate is organised as a platform-independent core plus a thin,
//! `std`-gated unix backend:
//!
//! ## Core (always available, `#![no_std]` + `alloc`)
//! * [`geom`] — points, sizes and rectangles.
//! * [`style`] — colors and text attributes.
//! * [`buffer`] — a grid of styled cells plus minimal-diff computation.
//! * [`event`] — decoded input events (keys, mouse, resize, paste).
//! * [`input`] — an incremental parser turning terminal byte streams into events.
//! * [`ansi`] — a capability model and builder for terminal escape sequences.
//! * [`layout`] — a constraint solver that splits regions into sub-regions.
//! * [`widget`] — the [`Widget`](widget::Widget) trait and a set of ready widgets.
//! * [`lineedit`] — a reusable single-line editor (the "line editor").
//!
//! ## Backend (`std` feature, on by default)
//! * [`backend`] — the [`Backend`](backend::Backend) trait and the unix TTY
//!   implementation (raw mode, alternate screen, mouse reporting).
//! * [`terminal`] — the [`Terminal`](terminal::Terminal) driver: double-buffered
//!   frame rendering built on [`buffer::Buffer::diff`].
//! * [`events`] — a background thread that reads the TTY and delivers
//!   [`Event`]s over a channel, with blocking and timed polling.
//!
//! ## Quick start
//! ```no_run
//! # #[cfg(feature = "std")] fn main() -> std::io::Result<()> {
//! use noroi::terminal::Terminal;
//! use noroi::widget::{Paragraph, Widget};
//! use noroi::event::{Event, KeyCode};
//!
//! let mut term = Terminal::open()?;      // raw mode + alternate screen
//! loop {
//!     term.draw(|frame| {
//!         Paragraph::new("Hello, noroi!  (press q to quit)")
//!             .render(frame.area(), frame.buffer_mut());
//!     })?;
//!     if let Some(Event::Key(k)) = term.events().poll(None)? {
//!         if k.code == KeyCode::Char('q') { break; }
//!     }
//! }
//! # Ok(()) }
//! # #[cfg(not(feature = "std"))] fn main() {}
//! ```
//! On drop, [`Terminal`](terminal::Terminal) restores the terminal to its
//! previous state (cooked mode, main screen, cursor visible), even on panic.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(not(feature = "std"), forbid(unsafe_code))]

extern crate alloc;

pub mod anim;
pub mod ansi;
pub mod buffer;
pub mod event;
pub mod geom;
pub mod input;
pub mod layout;
pub mod lineedit;
pub mod style;
pub mod theme;
pub mod widget;

#[cfg(feature = "std")]
pub mod backend;
#[cfg(feature = "std")]
pub mod events;
#[cfg(feature = "capi")]
pub mod ffi;
#[cfg(feature = "std")]
mod sys;
#[cfg(feature = "std")]
pub mod terminal;

pub use anim::{Easing, Pulse, Tween};
pub use buffer::{Buffer, Cell};
pub use event::{Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind};
pub use geom::{Point, Rect, Size};
pub use style::{Attributes, Color, Style};
pub use theme::Theme;
pub use widget::Widget;

#[cfg(feature = "std")]
pub use terminal::Terminal;
