# noroi

**Rich terminal UI for Rust, in the spirit of curses/ncurses — with zero external crate dependencies.**

noroi uses only `core`, `alloc`, and (for the OS-facing layer) `std`. No `libc`
crate, no `termios` crate, no `unicode-width` crate — nothing from crates.io. It
still gives you full-screen, mouse-aware, colorful terminal interfaces, from a
one-shot text view up to a multi-panel, animated, threaded application.

```
┌─ noroi ──────────────────────────────────────────────┐
│ ╭ Sections ─╮  ┌ Paragraph ─────────────────────────┐ │
│ │▶ Overview │  │ Word-wrapped, styled text rendered  │ │
│ │  Widgets  │  │ into a diffed cell buffer…          │ │
│ │  Layout   │  ├ Gauge (+/-) ───────────────────────┤ │
│ │  …        │  │ ████████████▍               61%     │ │
│ ╰───────────╯  │ [ Text input________ ]  [OK] [Cancel]│ │
└───────────────────────────────────────────────────────┘
```

## Highlights

- **Zero crates.** Raw mode and window size come from the C library `std`
  already links, declared with `extern "C"` — so the dependency graph is empty.
- **`no_std` core.** Everything platform-independent (buffers, styling, input
  parsing, layout, widgets, the line editor) builds with `--no-default-features`
  against just `core` + `alloc`, ready for embedding.
- **Flicker-free rendering.** Widgets paint into an off-screen cell grid; only
  the cells that changed between frames are written — the curses approach.
- **Full input.** An incremental parser decodes keys (with Ctrl/Alt/Shift),
  SGR + legacy mouse (click, drag, wheel), bracketed paste, focus events and
  UTF-8, coping with sequences split across reads.
- **Colors.** 16 / 256 / 24-bit true color, downgraded automatically to the
  terminal's detected depth.
- **Layout engine.** A constraint solver plus ergonomic `row`/`column`/`grid`/
  `spacer` helpers.
- **Widgets.** `Block` (four border styles, titles, padding), `Paragraph`
  (word wrap, scroll, alignment), `List` (selectable, scrollable), `Gauge`
  (sub-cell precision), `Button`, `Clear` (for popups), and a reusable
  `LineEditor` with history and emacs-style keybindings.
- **Animation.** Clock-free easing, [`Tween`] and [`Pulse`] primitives plus a
  [`Spinner`] widget: the app feeds a time delta each frame, animators yield a
  value, widgets stay pure. Honors reduced-motion.
- **Threaded events.** Input is read on a background thread and delivered over a
  channel; resize is detected without a `SIGWINCH` handler.
- **Theming.** A [`Theme`] collects a UI's style *roles* (text, accent, border,
  selection…) into one value, so a whole app shares an identity and re-skins by
  swapping a single theme. Ships with `ofuda` (the default) and `mono`.
- **C bindings.** An optional C ABI (`capi` feature) with a hand-written header.

## Quick start (Rust)

```toml
[dependencies]
noroi = "0.1"
```

```rust,no_run
use noroi::terminal::Terminal;
use noroi::widget::{Block, Borders, Paragraph, Wrap};
use noroi::event::{Event, KeyCode};

fn main() -> std::io::Result<()> {
    let mut term = Terminal::open()?;        // raw mode + alternate screen
    loop {
        term.draw(|frame| {
            let block = Block::bordered().borders(Borders::ALL).title("noroi");
            let inner = block.inner(frame.area());
            frame.render_widget(&block, frame.area());
            frame.render_widget(
                &Paragraph::new("Hello, noroi!  (press q to quit)").wrap(Wrap { trim: true }),
                inner,
            );
        })?;
        if let Some(Event::Key(k)) = term.events().poll(None)? {
            if k.code == KeyCode::Char('q') { break; }
        }
    }
    Ok(()) // Terminal restores the screen on drop, even on panic.
}
```

Run the bundled showcase of every widget:

```sh
cargo run --bin noroidemo
```

## Architecture

```
core (no_std + alloc)                     backend (std feature, unix)
  geom · style · buffer(+diff)              sys  (termios/winsize via extern "C")
  event · input(parser) · ansi              backend (Backend trait + UnixBackend)
  layout · widget · lineedit                events (threaded reader → channel)
                                            terminal (Terminal + Frame)
```

Build just the core for embedding:

```sh
cargo build --no-default-features
```

## Theming

Widgets take explicit styles, but a [`Theme`] gives a whole app one identity you
can swap at runtime. The default `ofuda` theme — sumi ink, washi paper and a
vermilion seal, after noroi's namesake 呪い ("curse") — is what `noroidemo`
wears; press `m` in the demo to re-skin it as `mono` (colorless, for monochrome
or `TERM=dumb`). Panels thicken and turn vermilion when focused.

```rust
use noroi::theme::Theme;

let theme = Theme::ofuda();                       // or Theme::mono()
let panel = theme.panel(is_focused).title("Sections");
// role styles: theme.text, theme.accent, theme.selection, theme.title, …
```

## Animation

The `#![no_std]` core has no clock, so animation is *driven*: the app measures
the seconds between frames and calls `advance(dt)` on each animator, which yields
a value the app hands to an otherwise-pure widget. This keeps rendering diffable
and makes motion (and reduced-motion) trivial to control.

```rust
use noroi::anim::{Easing, Tween};

let mut gauge = Tween::new(0.0, 0.6, 1.4, Easing::EaseInOutCubic);
// each frame:
gauge.advance(dt);              // dt = seconds since last frame
gauge.retarget(0.9);            // glide smoothly to a new value
let ratio = gauge.value();      // feed to Gauge::ratio(..)
```

`Pulse` is a looping breathe/blink oscillator, and [`Spinner`] is a frame-based
busy indicator (`Spinner::DOTS`, `LINE`, `ARC`, `CIRCLE`, `BAR`). In `noroidemo`
the gauge eases to its target, a spinner marks the live loop, and the focused
panel border breathes; `NOROI_REDUCED_MOTION=1` holds it all still.

## C bindings

Enable the `capi` feature and build a shared/static library (or use the
`Makefile`):

```sh
make capi          # → target/release/libnoroi.{so,a}
make cdemo         # builds examples/demo.c against it
```

```c
#include "noroi.h"
noroi_terminal *t = noroi_open();
noroi_begin(t);
noroi_box(t, 0, 0, 20, 3, NOROI_BORDER_ROUNDED, "Hi",
          noroi_color_indexed(NOROI_CYAN), noroi_color_default(), 0);
noroi_end(t);
noroi_event ev;
if (noroi_poll_event(t, 100, &ev) == 1 && ev.ch == 'q') { /* … */ }
noroi_close(t);
```

The full API is documented in [`include/noroi.h`](include/noroi.h).

## Platform support

The core is platform-independent. The backend targets Linux/Android today; other
unixes need only their `struct termios` layout added (see `src/sys/unix.rs`).

## License

MIT © Karpelès Lab Inc.
