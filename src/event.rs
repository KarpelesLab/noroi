//! Decoded input events.
//!
//! These types are what [`input::Parser`](crate::input::Parser) produces from a
//! raw terminal byte stream, and what a running UI consumes. They are backend
//! independent: the unix reader and any future backend both yield the same
//! [`Event`] enum.

use alloc::string::String;

/// A single input event delivered to the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse action occurred.
    Mouse(MouseEvent),
    /// The terminal was resized to `(columns, rows)`.
    Resize(u16, u16),
    /// A bracketed-paste payload arrived as a single chunk.
    Paste(String),
    /// The terminal window gained focus (requires focus reporting).
    FocusGained,
    /// The terminal window lost focus (requires focus reporting).
    FocusLost,
}

/// Keyboard modifier flags, as a bitset.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifiers held.
    pub const NONE: Modifiers = Modifiers(0);
    /// The Shift key.
    pub const SHIFT: Modifiers = Modifiers(1 << 0);
    /// The Alt / Meta key.
    pub const ALT: Modifiers = Modifiers(1 << 1);
    /// The Control key.
    pub const CTRL: Modifiers = Modifiers(1 << 2);

    /// True when every modifier in `other` is present.
    pub const fn contains(self, other: Modifiers) -> bool {
        (self.0 & other.0) == other.0
    }

    /// True when no modifier is held.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Raw bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Build modifiers from the numeric parameter xterm encodes (`1` means
    /// none, `2` shift, `3` alt, …); the value has already had 1 subtracted.
    pub(crate) fn from_xterm_mask(mask: u8) -> Modifiers {
        let mut m = Modifiers::NONE;
        if mask & 0b001 != 0 {
            m |= Modifiers::SHIFT;
        }
        if mask & 0b010 != 0 {
            m |= Modifiers::ALT;
        }
        if mask & 0b100 != 0 {
            m |= Modifiers::CTRL;
        }
        m
    }
}

impl core::ops::BitOr for Modifiers {
    type Output = Modifiers;
    fn bitor(self, rhs: Modifiers) -> Modifiers {
        Modifiers(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Modifiers) {
        self.0 |= rhs.0;
    }
}

impl core::fmt::Debug for Modifiers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut parts = [""; 3];
        let mut n = 0;
        if self.contains(Self::CTRL) {
            parts[n] = "CTRL";
            n += 1;
        }
        if self.contains(Self::ALT) {
            parts[n] = "ALT";
            n += 1;
        }
        if self.contains(Self::SHIFT) {
            parts[n] = "SHIFT";
            n += 1;
        }
        if n == 0 {
            f.write_str("NONE")
        } else {
            f.write_str(&parts[..n].join("+"))
        }
    }
}

/// A key press: which key, plus the modifiers held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// The logical key.
    pub code: KeyCode,
    /// Modifiers held during the press.
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// A key with no modifiers.
    pub const fn new(code: KeyCode) -> Self {
        KeyEvent { code, modifiers: Modifiers::NONE }
    }

    /// A key with the given modifiers.
    pub const fn with(code: KeyCode, modifiers: Modifiers) -> Self {
        KeyEvent { code, modifiers }
    }

    /// True if this is a bare `char` press with no modifiers (or only shift).
    pub fn is_char(&self, c: char) -> bool {
        self.code == KeyCode::Char(c)
    }

    /// True if Ctrl is held.
    pub fn ctrl(&self) -> bool {
        self.modifiers.contains(Modifiers::CTRL)
    }

    /// True if Alt is held.
    pub fn alt(&self) -> bool {
        self.modifiers.contains(Modifiers::ALT)
    }
}

/// A logical keyboard key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A printable character (already decoded from UTF-8).
    Char(char),
    /// Enter / Return.
    Enter,
    /// Tab.
    Tab,
    /// Shift-Tab (back-tab).
    BackTab,
    /// Backspace.
    Backspace,
    /// Escape.
    Esc,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert.
    Insert,
    /// Delete (forward delete).
    Delete,
    /// A function key `F(n)`, `n` starting at 1.
    F(u8),
    /// Null / an unrecognized control byte.
    Null,
}

/// A mouse action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    /// What happened.
    pub kind: MouseKind,
    /// Zero-based column.
    pub column: u16,
    /// Zero-based row.
    pub row: u16,
    /// Modifiers held during the action.
    pub modifiers: Modifiers,
}

/// The kind of a [`MouseEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseKind {
    /// A button was pressed.
    Down(MouseButton),
    /// A button was released.
    Up(MouseButton),
    /// The mouse moved while a button was held.
    Drag(MouseButton),
    /// The mouse moved with no button held.
    Moved,
    /// The wheel scrolled up.
    ScrollUp,
    /// The wheel scrolled down.
    ScrollDown,
    /// The wheel scrolled left.
    ScrollLeft,
    /// The wheel scrolled right.
    ScrollRight,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Left / primary button.
    Left,
    /// Middle button.
    Middle,
    /// Right / secondary button.
    Right,
}
