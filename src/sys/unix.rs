//! Unix TTY primitives, implemented against the C library that `std` already
//! links (so no external crate is required).
//!
//! The `struct termios` and `winsize` layouts and the flag constants here are
//! the Linux/Android ABI. Other unixes differ; supporting them is a matter of
//! adding the right `#[cfg]`-gated definitions.

#![allow(non_camel_case_types)]

#[cfg(not(any(target_os = "linux", target_os = "android")))]
compile_error!("noroi's unix backend currently targets Linux/Android ABI; \
other unixes need their own termios layout (contributions welcome)");

use core::ffi::{c_int, c_uchar, c_uint, c_ulong, c_void};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};

type tcflag_t = c_uint;
type cc_t = c_uchar;
type speed_t = c_uint;

const NCCS: usize = 32;

/// Mirror of Linux `struct termios`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Termios {
    c_iflag: tcflag_t,
    c_oflag: tcflag_t,
    c_cflag: tcflag_t,
    c_lflag: tcflag_t,
    c_line: cc_t,
    c_cc: [cc_t; NCCS],
    c_ispeed: speed_t,
    c_ospeed: speed_t,
}

impl Termios {
    fn zeroed() -> Termios {
        // All-zero is a valid (if meaningless) termios; it is immediately
        // overwritten by tcgetattr before use.
        Termios {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: 0,
            c_line: 0,
            c_cc: [0; NCCS],
            c_ispeed: 0,
            c_ospeed: 0,
        }
    }
}

/// Mirror of `struct winsize`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

unsafe extern "C" {
    fn tcgetattr(fd: c_int, termios_p: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, optional_actions: c_int, termios_p: *const Termios) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

// termios constants (Linux ABI).
const TCSAFLUSH: c_int = 2;
const TIOCGWINSZ: c_ulong = 0x5413;

const IGNBRK: tcflag_t = 0x0001;
const BRKINT: tcflag_t = 0x0002;
const PARMRK: tcflag_t = 0x0008;
const ISTRIP: tcflag_t = 0x0020;
const INLCR: tcflag_t = 0x0040;
const IGNCR: tcflag_t = 0x0080;
const ICRNL: tcflag_t = 0x0100;
const IXON: tcflag_t = 0x0400;

const OPOST: tcflag_t = 0x0001;

const ECHO: tcflag_t = 0x0008;
const ECHONL: tcflag_t = 0x0040;
const ICANON: tcflag_t = 0x0002;
const ISIG: tcflag_t = 0x0001;
const IEXTEN: tcflag_t = 0x8000;

const CSIZE: tcflag_t = 0x0030;
const PARENB: tcflag_t = 0x0100;
const CS8: tcflag_t = 0x0030;

const VTIME: usize = 5;
const VMIN: usize = 6;

/// The controlling terminal, opened read/write.
///
/// Prefer `/dev/tty` so the UI works even when stdin/stdout are redirected.
#[derive(Debug)]
pub struct Tty {
    file: File,
}

impl Tty {
    /// Clone the underlying handle (shares the same open file description, and
    /// thus the same terminal modes). Used to hand a reader to a thread.
    pub fn try_clone(&self) -> io::Result<Tty> {
        Ok(Tty { file: self.file.try_clone()? })
    }

    /// The raw file descriptor.
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl io::Read for Tty {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl io::Write for Tty {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Open the controlling terminal (`/dev/tty`) for reading and writing.
pub fn open_tty() -> io::Result<Tty> {
    let file = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    // Confirm it really is a terminal by fetching its attributes.
    let mut t = Termios::zeroed();
    let rc = unsafe { tcgetattr(file.as_raw_fd(), &mut t) };
    if rc != 0 {
        return Err(io::Error::new(io::ErrorKind::Other, "/dev/tty is not a terminal"));
    }
    Ok(Tty { file })
}

/// Query the terminal's size as `(columns, rows)`.
pub fn window_size(fd: RawFd) -> io::Result<(u16, u16)> {
    let mut ws = Winsize::default();
    let rc = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut Winsize as *mut c_void) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((ws.ws_col, ws.ws_row))
}

/// An RAII guard that puts the terminal into raw mode and restores the previous
/// mode when dropped — even on panic or early return.
#[derive(Debug)]
pub struct RawModeGuard {
    fd: RawFd,
    original: Termios,
}

impl RawModeGuard {
    /// Enter raw mode on `fd`, saving the current attributes for restoration.
    pub fn new(fd: RawFd) -> io::Result<RawModeGuard> {
        let mut original = Termios::zeroed();
        if unsafe { tcgetattr(fd, &mut original) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut raw = original;
        make_raw(&mut raw);
        if unsafe { tcsetattr(fd, TCSAFLUSH, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(RawModeGuard { fd, original })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // Best effort: nothing useful to do if restoration fails during unwind.
        unsafe {
            tcsetattr(self.fd, TCSAFLUSH, &self.original);
        }
    }
}

/// Apply the standard `cfmakeraw` transformation in place.
fn make_raw(t: &mut Termios) {
    t.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    t.c_oflag &= !OPOST;
    t.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
    t.c_cflag &= !(CSIZE | PARENB);
    t.c_cflag |= CS8;
    // Return from read() after 100ms even with no input, so the event loop can
    // resolve a lone ESC and poll for resize without a separate poll() call.
    t.c_cc[VMIN] = 0;
    t.c_cc[VTIME] = 1;
}
