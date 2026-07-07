//! `noroidemo` — an interactive showcase of every noroi widget.
//!
//! Run it with `cargo run --bin noroidemo`. It draws a dashboard exercising the
//! layout engine, [`Block`], [`Paragraph`], [`List`], [`Gauge`], [`Button`], the
//! [`LineEditor`], and a floating dialog (built from [`Clear`] + a [`Block`]).
//!
//! Controls:
//! * `Tab` / `Shift-Tab` — move focus between the list, the text field and the buttons.
//! * `↑`/`↓` — move the list selection (when the list is focused).
//! * type — edit the text field (when it is focused); `Enter` submits it.
//! * `+` / `-` — nudge the progress gauge; it also animates on its own.
//! * `Enter` / click — activate the focused button, or click any control.
//! * `d` — toggle a modal dialog; `?` — toggle the help overlay.
//! * `q` / `Ctrl-C` / `Esc` (twice) — quit.

use noroi::event::{Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind};
use noroi::geom::{Point, Rect};
use noroi::layout::{Constraint, column, row};
use noroi::lineedit::{LineEditor, LineOutcome};
use noroi::style::{Attributes, Color, Style};
use noroi::terminal::{Frame, Terminal};
use noroi::widget::{
    Block, BorderType, Button, Clear, Gauge, Line, List, ListItem, ListState, Padding, Paragraph,
    Span, Wrap,
};
use std::io;
use std::time::Duration;

/// Which control currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Input,
    ButtonOk,
    ButtonCancel,
}

impl Focus {
    const ORDER: [Focus; 4] = [
        Focus::List,
        Focus::Input,
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

/// Precomputed rectangles for one frame, reused for hit-testing.
struct Regions {
    title: Rect,
    sidebar: Rect,
    paragraph: Rect,
    gauge: Rect,
    input: Rect,
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
    let body = row([Constraint::Percentage(32), Constraint::Fill(1)])
        .spacing(1)
        .split(rows[1]);
    let main = column([
        Constraint::Fill(1),   // paragraph
        Constraint::Length(3), // gauge
        Constraint::Length(3), // input
        Constraint::Length(3), // buttons
    ])
    .split(body[1]);
    let buttons = row([Constraint::Fill(1), Constraint::Fill(1)])
        .spacing(2)
        .split(main[3]);
    Regions {
        title: rows[0],
        sidebar: body[0],
        paragraph: main[0],
        gauge: main[1],
        input: main[2],
        button_ok: buttons[0],
        button_cancel: buttons[1],
        status: rows[2],
    }
}

struct App {
    focus: Focus,
    list_state: ListState,
    items: Vec<String>,
    editor: LineEditor,
    gauge: f32,
    gauge_dir: f32,
    status: String,
    show_dialog: bool,
    show_help: bool,
    last_mouse: Option<(u16, u16)>,
    pending_esc: bool,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let mut list_state = ListState::new();
        list_state.select(Some(0));
        App {
            focus: Focus::List,
            list_state,
            items: [
                "Overview",
                "Widgets",
                "Layout engine",
                "Input & mouse",
                "Line editor",
                "Colors & styles",
                "Full-screen mode",
                "Dialogs & popups",
                "Threaded events",
                "C bindings",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            editor: LineEditor::new(),
            gauge: 0.42,
            gauge_dir: 0.01,
            status: "Welcome to noroi — press Tab to move focus, q to quit.".to_string(),
            show_dialog: false,
            show_help: false,
            last_mouse: None,
            pending_esc: false,
            should_quit: false,
        }
    }

    fn tick(&mut self) {
        // Ping-pong the gauge to demonstrate smooth sub-cell animation.
        self.gauge += self.gauge_dir;
        if self.gauge >= 1.0 {
            self.gauge = 1.0;
            self.gauge_dir = -self.gauge_dir.abs();
        } else if self.gauge <= 0.0 {
            self.gauge = 0.0;
            self.gauge_dir = self.gauge_dir.abs();
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        // Global shortcuts first.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(Modifiers::CTRL) {
            self.should_quit = true;
            return;
        }
        // Escape: close overlays, else require a double-press to quit.
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
                self.status = "Press Esc again to quit.".to_string();
            }
            return;
        }
        self.pending_esc = false;

        if self.show_dialog || self.show_help {
            // Any key dismisses the overlay.
            self.show_dialog = false;
            self.show_help = false;
            return;
        }

        // When the text field is focused, route printable keys to the editor.
        if self.focus == Focus::Input {
            match self.editor.handle_key(key) {
                LineOutcome::Submitted => {
                    self.status = format!("Submitted: {:?}", self.editor.text());
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
            KeyCode::Char('q') if self.focus != Focus::Input => self.should_quit = true,
            KeyCode::Char('d') if self.focus != Focus::Input => self.show_dialog = true,
            KeyCode::Char('?') if self.focus != Focus::Input => self.show_help = true,
            KeyCode::Tab => self.focus = self.focus.cycle(true),
            KeyCode::BackTab => self.focus = self.focus.cycle(false),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.gauge = (self.gauge + 0.05).min(1.0);
            }
            KeyCode::Char('-') => {
                self.gauge = (self.gauge - 0.05).max(0.0);
            }
            KeyCode::Up if self.focus == Focus::List => self.list_state.previous(self.items.len()),
            KeyCode::Down if self.focus == Focus::List => self.list_state.next(self.items.len()),
            KeyCode::Enter => self.activate_focus(),
            _ => {}
        }
    }

    fn activate_focus(&mut self) {
        match self.focus {
            Focus::ButtonOk => self.status = "OK pressed ✓".to_string(),
            Focus::ButtonCancel => self.status = "Cancel pressed ✗".to_string(),
            Focus::List => {
                if let Some(i) = self.list_state.selected() {
                    self.status = format!("Selected: {}", self.items[i]);
                }
            }
            Focus::Input => {}
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
                } else if r.input.contains(at) {
                    self.focus = Focus::Input;
                } else if r.sidebar.contains(at) {
                    self.focus = Focus::List;
                    // Map the click row to a list item.
                    let inner_top = r.sidebar.y + 1; // account for the block border
                    if at.y >= inner_top {
                        let idx = (at.y - inner_top) as usize + self.list_state.offset();
                        if idx < self.items.len() {
                            self.list_state.select(Some(idx));
                            self.activate_focus();
                        }
                    }
                }
            }
            MouseKind::ScrollDown => self.list_state.next(self.items.len()),
            MouseKind::ScrollUp => self.list_state.previous(self.items.len()),
            _ => {}
        }
    }
}

fn theme_title() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::LightCyan)
        .attrs(Attributes::BOLD)
}

fn accent() -> Color {
    Color::LightCyan
}

fn draw(frame: &mut Frame<'_>, app: &mut App, r: &Regions) {
    let area = frame.area();
    // Paint an overall background.
    frame.render_widget(&Clear::new().style(Style::new().bg(Color::Reset)), area);

    // Title bar.
    let title = Paragraph::new(Line::from_spans([
        Span::styled(" noroi ", theme_title()),
        Span::styled(
            "  terminal UI showcase",
            Style::new().fg(accent()).attrs(Attributes::BOLD),
        ),
        Span::raw("   —   Tab: focus   d: dialog   ?: help   q: quit"),
    ]))
    .style(Style::new().bg(Color::DarkGray).fg(Color::Gray));
    frame.render_widget(&title, r.title);

    // Sidebar list.
    let focused_list = app.focus == Focus::List;
    let list = List::new(app.items.iter().map(|s| ListItem::new(s.as_str())))
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(border_style(focused_list))
                .title(Line::styled(" Sections ", title_style(focused_list))),
        )
        .highlight_style(
            Style::new()
                .fg(Color::Black)
                .bg(accent())
                .attrs(Attributes::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(&list, r.sidebar, &mut app.list_state);

    // Main paragraph inside a block.
    let selected = app
        .list_state
        .selected()
        .and_then(|i| app.items.get(i))
        .cloned()
        .unwrap_or_default();
    let body_text = format!(
        "This panel is a word-wrapped Paragraph inside a Block.\n\nThe currently selected \
         section is “{selected}”. noroi renders everything into an off-screen cell buffer and \
         writes only the cells that changed each frame, so redraws are cheap and flicker-free — \
         the same idea curses pioneered.\n\nEverything you see is drawn with zero external crates: \
         geometry, styling, the 16/256/true-color model, Unicode width handling, the input \
         parser, the layout engine and these widgets are all pure Rust.",
    );
    let para = Paragraph::new(body_text).wrap(Wrap { trim: true }).block(
        Block::bordered()
            .border_type(BorderType::Plain)
            .title(Line::raw(" Paragraph "))
            .padding(Padding::symmetric(1, 0)),
    );
    frame.render_widget(&para, r.paragraph);

    // Gauge.
    let gauge = Gauge::new()
        .ratio(app.gauge)
        .filled_style(Style::new().fg(Color::Black).bg(accent()))
        .unfilled_style(Style::new().fg(Color::Gray).bg(Color::DarkGray))
        .block(Block::bordered().title(Line::raw(" Gauge  (+/-) ")));
    frame.render_widget(&gauge, r.gauge);

    // Text input (line editor).
    let input_focused = app.focus == Focus::Input;
    let input_block = Block::bordered()
        .border_style(border_style(input_focused))
        .title(Line::styled(" Text input ", title_style(input_focused)));
    let input_inner = input_block.inner(r.input);
    frame.render_widget(&input_block, r.input);
    let cursor = app.editor.render(
        input_inner,
        frame.buffer_mut(),
        Style::new().fg(Color::White),
    );
    if input_focused {
        frame.set_cursor(cursor);
    }

    // Buttons.
    let ok = Button::new("OK  (Enter)").focused(app.focus == Focus::ButtonOk);
    let cancel = Button::new("Cancel")
        .style(Style::new().fg(Color::White).bg(Color::Red))
        .focus_style(
            Style::new()
                .fg(Color::White)
                .bg(Color::LightRed)
                .attrs(Attributes::BOLD),
        )
        .focused(app.focus == Focus::ButtonCancel);
    frame.render_widget(&ok, centered_button(r.button_ok));
    frame.render_widget(&cancel, centered_button(r.button_cancel));

    // Status bar.
    let mouse = app
        .last_mouse
        .map(|(x, y)| format!("  mouse@{x},{y}"))
        .unwrap_or_default();
    let status = Paragraph::new(Line::from_spans([
        Span::styled(" status ", Style::new().fg(Color::Black).bg(Color::Green)),
        Span::raw(" "),
        Span::raw(app.status.clone()),
        Span::styled(mouse, Style::new().fg(Color::DarkGray)),
    ]))
    .style(Style::new().bg(Color::Black).fg(Color::Gray));
    frame.render_widget(&status, r.status);

    // Overlays.
    if app.show_help {
        draw_help(frame, area);
    } else if app.show_dialog {
        draw_dialog(frame, area);
    }
}

fn draw_dialog(frame: &mut Frame<'_>, area: Rect) {
    let dialog = area.centered(noroi::geom::Size::new(44, 9));
    frame.render_widget(&Clear::new(), dialog);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::LightYellow))
        .style(Style::new().bg(Color::Blue))
        .title(Line::styled(
            " Dialog ",
            Style::new().fg(Color::White).attrs(Attributes::BOLD),
        ))
        .padding(Padding::uniform(1));
    let inner = block.inner(dialog);
    frame.render_widget(&block, dialog);
    let text = Paragraph::new(
        "This is a modal dialog: a Clear widget blanks the region, then a Block \
         draws over it. Press any key to dismiss.",
    )
    .wrap(Wrap { trim: true })
    .style(Style::new().fg(Color::White).bg(Color::Blue));
    frame.render_widget(&text, inner);
}

fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = area.centered(noroi::geom::Size::new(52, 14));
    frame.render_widget(&Clear::new(), popup);
    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::new().fg(Color::LightGreen))
        .style(Style::new().bg(Color::Black))
        .title(Line::styled(
            " Help ",
            Style::new().fg(Color::LightGreen).attrs(Attributes::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(&block, popup);
    let lines = [
        Line::from_spans([
            Span::styled("Tab / Shift-Tab", key_style()),
            Span::raw("  move focus"),
        ]),
        Line::from_spans([
            Span::styled("↑ / ↓", key_style()),
            Span::raw("          list selection"),
        ]),
        Line::from_spans([
            Span::styled("type / Enter", key_style()),
            Span::raw("   edit / submit text field"),
        ]),
        Line::from_spans([
            Span::styled("+ / -", key_style()),
            Span::raw("          adjust the gauge"),
        ]),
        Line::from_spans([
            Span::styled("mouse", key_style()),
            Span::raw("          click controls, scroll list"),
        ]),
        Line::from_spans([
            Span::styled("d / ?", key_style()),
            Span::raw("          dialog / this help"),
        ]),
        Line::from_spans([
            Span::styled("q / Ctrl-C", key_style()),
            Span::raw("     quit"),
        ]),
        Line::raw(""),
        Line::raw("Press any key to close.").centered(),
    ];
    let para = Paragraph::new(noroi::widget::Text::from_lines(lines))
        .style(Style::new().fg(Color::Gray).bg(Color::Black));
    frame.render_widget(&para, inner);
}

fn key_style() -> Style {
    Style::new().fg(Color::LightCyan).attrs(Attributes::BOLD)
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::new().fg(accent()).attrs(Attributes::BOLD)
    } else {
        Style::new().fg(Color::DarkGray)
    }
}

fn title_style(focused: bool) -> Style {
    if focused {
        Style::new().fg(accent()).attrs(Attributes::BOLD)
    } else {
        Style::new().fg(Color::Gray)
    }
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

    while !app.should_quit {
        let area = terminal.area();
        let r = regions(area);
        terminal.draw(|frame| draw(frame, &mut app, &r))?;

        // Wait up to a frame interval; a timeout drives the gauge animation.
        match terminal.events().poll(Some(Duration::from_millis(80)))? {
            Some(Event::Key(key)) => app.on_key(key),
            Some(Event::Mouse(m)) => app.on_mouse(m, &r),
            Some(Event::Paste(text)) => {
                if app.focus == Focus::Input {
                    app.editor.insert_str(&text);
                }
            }
            Some(Event::Resize(_, _)) => { /* buffers resize automatically on next draw */ }
            Some(_) => {}
            None => app.tick(),
        }
    }
    Ok(())
}
