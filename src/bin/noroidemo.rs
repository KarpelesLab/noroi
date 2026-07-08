//! `noroidemo` — an interactive showcase of every noroi widget.
//!
//! Run it with `cargo run --bin noroidemo`. It draws a dashboard exercising the
//! layout engine, [`Block`], [`Paragraph`], [`List`], [`Gauge`], [`Button`], the
//! [`LineEditor`], the [`Spinner`], a floating dialog, the [`Theme`] system and
//! the animation primitives.
//!
//! The look is "ofuda" — sumi ink, washi paper and a vermilion seal, after
//! noroi's namesake 呪い ("curse"). Press `m` to reskin it as the monochrome
//! theme; focus a panel and its border thickens, reddens and gently breathes.
//!
//! Motion: the gauge eases to its target instead of jumping, a spinner marks the
//! live loop, and the focused border pulses. Set `NOROI_REDUCED_MOTION=1` to
//! hold everything still.
//!
//! Controls:
//! * `Tab` / `Shift-Tab` — move focus between the list, the prompt and the buttons.
//! * `↑`/`↓` — move the list selection (when the list is focused).
//! * type — edit the prompt (when it is focused); `Enter` submits it.
//! * `+` / `-` — set the gauge target; it eases there.
//! * `Enter` / click — activate the focused button, or click any control.
//! * `m` — switch theme · `d` — dialog · `?` — help.
//! * `q` / `Ctrl-C` / `Esc` (twice) — quit.

use noroi::anim::{Easing, Pulse, Tween};
use noroi::event::{Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind};
use noroi::geom::{Point, Rect, Size};
use noroi::layout::{Constraint, column, row};
use noroi::lineedit::{LineEditor, LineOutcome};
use noroi::style::{Color, Style};
use noroi::terminal::{Frame, Terminal};
use noroi::theme::Theme;
use noroi::widget::{
    Alignment, Block, Button, Clear, Gauge, Line, List, ListItem, ListState, Padding, Paragraph,
    Span, Spinner, Text, Wrap,
};
use std::io;
use std::time::{Duration, Instant};

/// Which control currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Prompt,
    ButtonOk,
    ButtonCancel,
}

impl Focus {
    const ORDER: [Focus; 4] = [
        Focus::List,
        Focus::Prompt,
        Focus::ButtonOk,
        Focus::ButtonCancel,
    ];

    fn cycle(self, forward: bool) -> Focus {
        let idx = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        let len = Self::ORDER.len();
        let next = if forward {
            (idx + 1) % len
        } else {
            (idx + len - 1) % len
        };
        Self::ORDER[next]
    }
}

/// The sections shown in the sidebar, each with a one-line description that the
/// main panel shows when it is selected. Order is not meaningful, so the list is
/// not numbered.
const SECTIONS: &[(&str, &str)] = &[
    (
        "Overview",
        "noroi draws whole interfaces into an off-screen cell grid and writes only \
         the cells that change each frame — flicker-free, the way curses does it.",
    ),
    (
        "Widgets",
        "Blocks, paragraphs, lists, gauges, buttons, spinners and prompts. Each one \
         paints into a buffer and clips itself; none of them touch the terminal.",
    ),
    (
        "Layout engine",
        "Split any region with sizing constraints, or reach for the row, column, \
         grid and spacer helpers to place widgets without doing arithmetic.",
    ),
    (
        "Input & mouse",
        "One parser decodes keys and modifiers, SGR mouse (click, drag, wheel), \
         bracketed paste and focus — coping with sequences split across reads.",
    ),
    (
        "Line editor",
        "A reusable single-line editor with history and emacs-style keys. The \
         prompt below is one — focus it with Tab and type.",
    ),
    (
        "Animation",
        "Easing, tweens and pulses advance by a time delta the app supplies. The \
         gauge eases to its target; the spinner and focus border are live.",
    ),
    (
        "Colors & styles",
        "16, 256 and 24-bit color, downgraded to fit the terminal. Press m to \
         reskin this demo between the ofuda and mono themes.",
    ),
    (
        "Dialogs & popups",
        "Clear a region, then draw over it. Press d for a modal dialog, or ? for \
         the help overlay — both are built this way.",
    ),
    (
        "Threaded events",
        "Input is read on a background thread and delivered over a channel; a \
         resize is noticed without any signal handler.",
    ),
    (
        "C bindings",
        "A C ABI ships alongside: open a terminal, draw boxes and text, and poll \
         events from C. The header is include/noroi.h.",
    ),
];

/// Precomputed rectangles for one frame, reused for hit-testing.
struct Regions {
    title: Rect,
    sidebar: Rect,
    palette: Rect,
    paragraph: Rect,
    gauge: Rect,
    prompt: Rect,
    button_ok: Rect,
    button_cancel: Rect,
    status: Rect,
}

fn regions(area: Rect) -> Regions {
    let rows = column([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .split(area);
    // Inset the body by a column on each side for breathing room.
    let body_area = rows[1].shrink(1, 0);
    let body = row([Constraint::Length(24), Constraint::Fill(1)])
        .spacing(1)
        .split(body_area);
    // The sidebar stacks the section list above a small palette panel.
    let side = column([Constraint::Fill(1), Constraint::Length(6)]).split(body[0]);
    let main = column([
        Constraint::Fill(1),   // paragraph
        Constraint::Length(3), // gauge
        Constraint::Length(3), // prompt
        Constraint::Length(3), // buttons
    ])
    .split(body[1]);
    let buttons = row([Constraint::Fill(1), Constraint::Fill(1)])
        .spacing(2)
        .split(main[3]);
    Regions {
        title: rows[0],
        sidebar: side[0],
        palette: side[1],
        paragraph: main[0],
        gauge: main[1],
        prompt: main[2],
        button_ok: buttons[0],
        button_cancel: buttons[1],
        status: rows[2],
    }
}

struct App {
    theme: Theme,
    mono: bool,
    reduced_motion: bool,
    focus: Focus,
    list_state: ListState,
    editor: LineEditor,
    // Animation state.
    gauge: Tween,
    focus_pulse: Pulse,
    spinner: Spinner,
    auto_dwell: f32,
    manual_hold: f32,
    status: String,
    show_dialog: bool,
    show_help: bool,
    last_mouse: Option<(u16, u16)>,
    pending_esc: bool,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let reduced_motion = std::env::var_os("NOROI_REDUCED_MOTION").is_some();
        let mut list_state = ListState::new();
        list_state.select(Some(0));
        let gauge = if reduced_motion {
            Tween::settled(0.62)
        } else {
            // Reveal the gauge from empty on start.
            Tween::new(0.0, 0.62, 1.4, Easing::EaseInOutCubic)
        };
        App {
            theme: Theme::ofuda(),
            mono: false,
            reduced_motion,
            focus: Focus::List,
            list_state,
            editor: LineEditor::new(),
            gauge,
            focus_pulse: Pulse::new(1.6),
            spinner: Spinner::new().label("live"),
            auto_dwell: 0.0,
            manual_hold: 0.0,
            status: "Select a section, or press ? for help.".to_string(),
            show_dialog: false,
            show_help: false,
            last_mouse: None,
            pending_esc: false,
            should_quit: false,
        }
    }

    /// Advance every animation by `dt` seconds.
    fn advance(&mut self, dt: f32) {
        if self.reduced_motion {
            return;
        }
        self.spinner.advance(dt);
        self.focus_pulse.advance(dt);
        self.gauge.advance(dt);
        if self.manual_hold > 0.0 {
            self.manual_hold -= dt;
        } else if self.gauge.finished() {
            // Auto-cycle the target after a short dwell, to show easing.
            self.auto_dwell += dt;
            if self.auto_dwell > 0.7 {
                self.auto_dwell = 0.0;
                let next = if self.gauge.target() > 0.6 {
                    0.18
                } else {
                    0.92
                };
                self.gauge.retarget(next);
            }
        }
    }

    fn set_gauge_target(&mut self, target: f32) {
        let target = target.clamp(0.0, 1.0);
        if self.reduced_motion {
            self.gauge.snap(target);
        } else {
            self.gauge.retarget(target);
            self.manual_hold = 2.0; // pause the auto-cycle after manual input
        }
    }

    fn toggle_theme(&mut self) {
        self.mono = !self.mono;
        self.theme = if self.mono {
            Theme::mono()
        } else {
            Theme::ofuda()
        };
        self.status = format!("Theme: {}", if self.mono { "mono" } else { "ofuda" });
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(Modifiers::CTRL) {
            self.should_quit = true;
            return;
        }
        if key.code == KeyCode::Esc {
            if self.show_dialog || self.show_help {
                self.show_dialog = false;
                self.show_help = false;
                self.pending_esc = false;
                return;
            }
            if self.pending_esc {
                self.should_quit = true;
            } else {
                self.pending_esc = true;
                self.status = "Press Esc again to leave.".to_string();
            }
            return;
        }
        self.pending_esc = false;

        if self.show_dialog || self.show_help {
            self.show_dialog = false;
            self.show_help = false;
            return;
        }

        // When the prompt is focused, printable keys edit it.
        if self.focus == Focus::Prompt {
            match self.editor.handle_key(key) {
                LineOutcome::Submitted => {
                    self.status = format!("Prompt: {:?}", self.editor.text());
                    self.editor.clear();
                    return;
                }
                LineOutcome::Changed => return,
                LineOutcome::Cancelled => {
                    self.focus = Focus::List;
                    return;
                }
                LineOutcome::Ignored => {}
            }
        }

        match key.code {
            KeyCode::Char('q') if self.focus != Focus::Prompt => self.should_quit = true,
            KeyCode::Char('d') if self.focus != Focus::Prompt => self.show_dialog = true,
            KeyCode::Char('?') if self.focus != Focus::Prompt => self.show_help = true,
            KeyCode::Char('m') if self.focus != Focus::Prompt => self.toggle_theme(),
            KeyCode::Tab => self.focus = self.focus.cycle(true),
            KeyCode::BackTab => self.focus = self.focus.cycle(false),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.set_gauge_target(self.gauge.target() + 0.08)
            }
            KeyCode::Char('-') => self.set_gauge_target(self.gauge.target() - 0.08),
            KeyCode::Up if self.focus == Focus::List => self.list_state.previous(SECTIONS.len()),
            KeyCode::Down if self.focus == Focus::List => self.list_state.next(SECTIONS.len()),
            KeyCode::Enter => self.activate_focus(),
            _ => {}
        }
    }

    fn activate_focus(&mut self) {
        match self.focus {
            Focus::ButtonOk => self.status = "Applied.".to_string(),
            Focus::ButtonCancel => self.status = "Dismissed.".to_string(),
            Focus::List => {
                if let Some(i) = self.list_state.selected() {
                    self.status = format!("▸ {}", SECTIONS[i].0);
                }
            }
            Focus::Prompt => {}
        }
    }

    fn on_mouse(&mut self, ev: MouseEvent, r: &Regions) {
        self.last_mouse = Some((ev.column, ev.row));
        let at = Point::new(ev.column, ev.row);
        match ev.kind {
            MouseKind::Down(MouseButton::Left) => {
                if r.button_ok.contains(at) {
                    self.focus = Focus::ButtonOk;
                    self.activate_focus();
                } else if r.button_cancel.contains(at) {
                    self.focus = Focus::ButtonCancel;
                    self.activate_focus();
                } else if r.prompt.contains(at) {
                    self.focus = Focus::Prompt;
                } else if r.sidebar.contains(at) {
                    self.focus = Focus::List;
                    let inner_top = r.sidebar.y + 1; // past the block's top border
                    if at.y >= inner_top {
                        let idx = (at.y - inner_top) as usize + self.list_state.offset();
                        if idx < SECTIONS.len() {
                            self.list_state.select(Some(idx));
                            self.activate_focus();
                        }
                    }
                }
            }
            MouseKind::ScrollDown => self.list_state.next(SECTIONS.len()),
            MouseKind::ScrollUp => self.list_state.previous(SECTIONS.len()),
            _ => {}
        }
    }

    /// A themed panel whose focused border gently pulses (ofuda only).
    fn panel(&self, focused: bool) -> Block {
        let block = self.theme.panel(focused);
        if focused && !self.mono && !self.reduced_motion {
            let t = self.focus_pulse.value();
            // Breathe between vermilion and a warmer ember.
            let color = Color::Rgb(193, 39, 45).lerp(Color::Rgb(226, 138, 96), t);
            block.border_style(Style::new().fg(color).bold())
        } else {
            block
        }
    }
}

fn draw(frame: &mut Frame<'_>, app: &mut App, r: &Regions) {
    let t = app.theme;
    let area = frame.area();
    frame.render_widget(&Clear::new().style(t.background), area);

    draw_title(frame, app, r.title);
    draw_sidebar(frame, app, r);
    draw_palette(frame, t, r.palette);
    draw_paragraph(frame, app, r);
    draw_gauge(frame, app, r);
    draw_prompt(frame, app, r);
    draw_buttons(frame, app, r);
    draw_status(frame, app, r);

    if app.show_help {
        draw_help(frame, t, area);
    } else if app.show_dialog {
        draw_dialog(frame, t, area);
    }
}

/// The signature: a stamped hanko seal and gold tagline, with a live spinner
/// pinned to the right edge.
fn draw_title(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let t = app.theme;
    let title = Line::from_spans([
        Span::styled(" 呪 noroi ", t.selection),
        Span::raw("  "),
        Span::styled("the terminal, cursed", t.accent_alt),
    ]);
    frame.render_widget(&Paragraph::new(title).style(t.background), area);

    let spinner = app.spinner.clone().style(t.accent).label_style(t.dim);
    let w = spinner.width();
    if area.width > w + 1 {
        let x = area.right() - w - 1;
        frame.render_widget(&spinner, Rect::new(x, area.y, w, 1));
    }
}

fn draw_sidebar(frame: &mut Frame<'_>, app: &mut App, r: &Regions) {
    let t = app.theme;
    let focused = app.focus == Focus::List;
    let block = app
        .panel(focused)
        .title(Line::styled(" Sections ", t.title));
    let items = SECTIONS.iter().map(|(name, _)| ListItem::new(*name));
    let list = List::new(items)
        .block(block)
        .style(t.text)
        .highlight_style(t.selection)
        .highlight_symbol("  ");
    frame.render_stateful_widget(&list, r.sidebar, &mut app.list_state);
}

/// A small panel of color swatches — fills the sidebar and shows the palette
/// the whole UI is derived from.
fn draw_palette(frame: &mut Frame<'_>, t: Theme, area: Rect) {
    let block = t.panel(false).title(Line::styled(" Palette ", t.title));
    let inner = block.inner(area);
    frame.render_widget(&block, area);
    let swatches = [
        ("朱 vermilion", t.accent),
        ("金 gold", t.accent_alt),
        ("紙 paper", t.text),
        ("墨 ink", t.border),
    ];
    for (i, (label, style)) in swatches.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.bottom() {
            break;
        }
        let line = Line::from_spans([
            Span::styled("██", *style),
            Span::styled(format!(" {label}"), t.dim),
        ]);
        frame.render_widget(&Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
    }
}

fn draw_paragraph(frame: &mut Frame<'_>, app: &App, r: &Regions) {
    let t = app.theme;
    let (name, blurb) = app
        .list_state
        .selected()
        .map(|i| SECTIONS[i])
        .unwrap_or(SECTIONS[0]);
    let block = t
        .panel(false)
        .title(Line::styled(format!(" {name} "), t.title))
        .padding(Padding::symmetric(1, 0));
    let body = Text::from_lines([
        Line::styled(blurb, t.text),
        Line::raw(""),
        Line::from_spans([
            Span::styled("zero external crates", t.accent),
            Span::styled("  ·  core + alloc + std only", t.dim),
        ]),
    ]);
    let para = Paragraph::new(body).wrap(Wrap { trim: true }).block(block);
    frame.render_widget(&para, r.paragraph);
}

fn draw_gauge(frame: &mut Frame<'_>, app: &App, r: &Regions) {
    let t = app.theme;
    let gauge = Gauge::new()
        .ratio(app.gauge.value())
        .filled_style(t.gauge_filled)
        .unfilled_style(t.gauge_unfilled)
        .block(
            t.panel(false)
                .title(Line::styled(" Gauge  (+/-) ", t.title)),
        );
    frame.render_widget(&gauge, r.gauge);
}

fn draw_prompt(frame: &mut Frame<'_>, app: &mut App, r: &Regions) {
    let t = app.theme;
    let focused = app.focus == Focus::Prompt;
    let block = app.panel(focused).title(Line::styled(
        " Prompt ",
        if focused { t.accent } else { t.title },
    ));
    let inner = block.inner(r.prompt);
    frame.render_widget(&block, r.prompt);
    let cursor = app.editor.render(inner, frame.buffer_mut(), t.text);
    if focused {
        frame.set_cursor(cursor);
    }
}

fn draw_buttons(frame: &mut Frame<'_>, app: &App, r: &Regions) {
    let t = app.theme;
    let ok = Button::new("Apply")
        .style(t.button)
        .focus_style(t.button_focused)
        .focused(app.focus == Focus::ButtonOk);
    let cancel = Button::new("Cancel")
        .style(t.button)
        .focus_style(t.button_focused)
        .focused(app.focus == Focus::ButtonCancel);
    frame.render_widget(&ok, centered_button(r.button_ok));
    frame.render_widget(&cancel, centered_button(r.button_cancel));
}

fn draw_status(frame: &mut Frame<'_>, app: &App, r: &Regions) {
    let t = app.theme;
    frame.render_widget(&Clear::new().style(t.background), r.status);
    let left = Line::from_spans([
        Span::styled(" 呪 ", t.selection),
        Span::raw(" "),
        Span::styled(app.status.clone(), t.text),
    ]);
    frame.render_widget(&Paragraph::new(left).style(t.background), r.status);

    let mouse = app
        .last_mouse
        .map(|(x, y)| format!("{x},{y}  "))
        .unwrap_or_default();
    let hints = Line::from_spans([
        Span::styled(mouse, t.dim),
        Span::styled("tab ↑↓ · m d ? · q ", t.dim),
    ])
    .alignment(Alignment::Right);
    frame.render_widget(&Paragraph::new(hints).style(t.background), r.status);
}

fn draw_dialog(frame: &mut Frame<'_>, t: Theme, area: Rect) {
    let dialog = area.centered(Size::new(46, 9));
    frame.render_widget(&Clear::new().style(t.background), dialog);
    let block = t
        .panel(true)
        .title(Line::styled(" Confirm ", t.title))
        .padding(Padding::uniform(1));
    let inner = block.inner(dialog);
    frame.render_widget(&block, dialog);
    let text = Text::from_lines([
        Line::styled("A modal dialog: Clear blanks the region, then a", t.text),
        Line::styled(
            "Block draws over it. Nothing behind bleeds through.",
            t.text,
        ),
        Line::raw(""),
        Line::from_spans([
            Span::styled("Press any key", t.accent),
            Span::styled(" to dismiss.", t.dim),
        ]),
    ]);
    frame.render_widget(&Paragraph::new(text).wrap(Wrap { trim: true }), inner);
}

fn draw_help(frame: &mut Frame<'_>, t: Theme, area: Rect) {
    let popup = area.centered(Size::new(52, 15));
    frame.render_widget(&Clear::new().style(t.background), popup);
    let block = t
        .panel(true)
        .title(Line::styled(" Help ", t.title))
        .padding(Padding::uniform(1));
    let inner = block.inner(popup);
    frame.render_widget(&block, popup);

    let key = |k: &str, desc: &str| {
        Line::from_spans([
            Span::styled(format!("{k:<14}"), t.accent),
            Span::styled(desc.to_string(), t.text),
        ])
    };
    let lines = [
        key("Tab / S-Tab", "move focus"),
        key("↑ / ↓", "list selection"),
        key("type / Enter", "edit / submit the prompt"),
        key("+ / -", "set the gauge target (it eases there)"),
        key("m", "switch theme (ofuda / mono)"),
        key("mouse", "click controls, scroll the list"),
        key("d / ?", "dialog / this help"),
        key("q / Ctrl-C", "quit"),
        Line::raw(""),
        Line::styled("Set NOROI_REDUCED_MOTION=1 to hold motion still.", t.dim),
        Line::styled("Press any key to close.", t.dim).alignment(Alignment::Center),
    ];
    frame.render_widget(&Paragraph::new(Text::from_lines(lines)), inner);
}

/// Center a one-row button vertically inside its 3-row cell.
fn centered_button(cell: Rect) -> Rect {
    if cell.height >= 3 {
        Rect::new(cell.x, cell.y + 1, cell.width, 1)
    } else {
        cell
    }
}

fn main() -> io::Result<()> {
    let mut terminal = Terminal::open()?;
    let mut app = App::new();
    let mut last = Instant::now();

    while !app.should_quit {
        let area = terminal.area();
        let r = regions(area);
        terminal.draw(|frame| draw(frame, &mut app, &r))?;

        // ~30 fps: a timeout drives the animations; input arrives in between.
        let event = terminal.events().poll(Some(Duration::from_millis(33)))?;
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32();
        last = now;
        app.advance(dt);

        match event {
            Some(Event::Key(key)) => app.on_key(key),
            Some(Event::Mouse(m)) => app.on_mouse(m, &r),
            Some(Event::Paste(text)) => {
                if app.focus == Focus::Prompt {
                    app.editor.insert_str(&text);
                }
            }
            Some(Event::Resize(_, _)) => {}
            Some(_) => {}
            None => {}
        }
    }
    Ok(())
}
