//! Operating-system facing primitives.
//!
//! This module is only compiled with the `std` feature. It isolates every
//! platform detail — raw mode, terminal size, opening the controlling TTY —
//! behind a small interface so the rest of the crate stays portable. Only unix
//! is implemented today; the interface is what a future backend (e.g. Windows
//! console) would re-implement.

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub use unix::{open_tty, window_size, RawModeGuard, Tty};

#[cfg(not(unix))]
compile_error!("the `std` feature of noroi currently supports unix targets only; \
build with --no-default-features for the platform-independent core");
