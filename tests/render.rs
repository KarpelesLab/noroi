//! End-to-end tests of the render + event pipeline using the headless
//! [`TestBackend`], so they run anywhere — no real terminal required.

#![cfg(feature = "std")]

use std::sync::mpsc;
use std::time::Duration;

use noroi::backend::TestBackend;
use noroi::event::{Event, KeyCode, KeyEvent};
use noroi::events::EventStream;
use noroi::geom::{Point, Rect, Size};
use noroi::layout::{Constraint, column};
use noroi::style::{Color, Style};
use noroi::terminal::Terminal;
use noroi::widget::{Block, Borders, Gauge, Paragraph, Widget};

fn harness(size: Size) -> (Terminal<TestBackend>, mpsc::Sender<Event>) {
    let (tx, rx) = mpsc::channel();
    let events = EventStream::from_receiver(rx);
    let terminal = Terminal::from_parts(TestBackend::new(size), events).unwrap();
    (terminal, tx)
}

#[test]
fn draws_paragraph_into_screen_buffer() {
    let (mut term, _tx) = harness(Size::new(20, 3));
    term.draw(|frame| {
        Paragraph::new("hello").render(Rect::new(0, 0, 20, 1), frame.buffer_mut());
    })
    .unwrap();

    let buf = term.current_buffer();
    let word: String = "hello"
        .char_indices()
        .map(|(i, _)| buf.get(i as u16, 0).unwrap().symbol_char())
        .collect();
    assert_eq!(word, "hello");
    // First frame paints every non-blank cell we touched.
    assert!(term.backend().cells_drawn >= 5);
}

#[test]
fn diff_only_redraws_changes() {
    let (mut term, _tx) = harness(Size::new(10, 1));
    term.draw(|frame| {
        Paragraph::new("aaaa").render(Rect::new(0, 0, 10, 1), frame.buffer_mut());
    })
    .unwrap();
    // Repaint an identical frame: nothing should change.
    term.draw(|frame| {
        Paragraph::new("aaaa").render(Rect::new(0, 0, 10, 1), frame.buffer_mut());
    })
    .unwrap();
    assert_eq!(term.backend().cells_drawn, 0);

    // Change one cell.
    term.draw(|frame| {
        Paragraph::new("aaba").render(Rect::new(0, 0, 10, 1), frame.buffer_mut());
    })
    .unwrap();
    assert_eq!(term.backend().cells_drawn, 1);
}

#[test]
fn block_borders_and_title_render() {
    let (mut term, _tx) = harness(Size::new(8, 3));
    term.draw(|frame| {
        let block = Block::bordered().borders(Borders::ALL).title("Hi");
        block.render(Rect::new(0, 0, 8, 3), frame.buffer_mut());
    })
    .unwrap();
    let buf = term.current_buffer();
    assert_eq!(buf.get(0, 0).unwrap().symbol_char(), '┌');
    assert_eq!(buf.get(7, 0).unwrap().symbol_char(), '┐');
    assert_eq!(buf.get(0, 2).unwrap().symbol_char(), '└');
    // Title starts one cell in from the corner.
    assert_eq!(buf.get(1, 0).unwrap().symbol_char(), 'H');
}

#[test]
fn gauge_fills_proportionally() {
    let (mut term, _tx) = harness(Size::new(10, 1));
    term.draw(|frame| {
        Gauge::new()
            .ratio(0.5)
            .label("")
            .filled_style(Style::new().bg(Color::Green))
            .render(Rect::new(0, 0, 10, 1), frame.buffer_mut());
    })
    .unwrap();
    let buf = term.current_buffer();
    // Left half full blocks, right half spaces.
    assert_eq!(buf.get(0, 0).unwrap().symbol_char(), '█');
    assert_eq!(buf.get(4, 0).unwrap().symbol_char(), '█');
    assert_eq!(buf.get(9, 0).unwrap().symbol_char(), ' ');
}

#[test]
fn cursor_is_positioned_and_toggled() {
    let (mut term, _tx) = harness(Size::new(10, 2));
    term.draw(|frame| frame.set_cursor(Point::new(3, 1)))
        .unwrap();
    assert_eq!(term.backend().cursor, Some((3, 1)));
    assert!(term.backend().cursor_visible);

    // A frame that sets no cursor hides it.
    term.draw(|_frame| {}).unwrap();
    assert!(!term.backend().cursor_visible);
}

#[test]
fn resize_triggers_full_repaint() {
    let (mut term, _tx) = harness(Size::new(5, 1));
    term.draw(|frame| {
        Paragraph::new("xxxxx").render(frame.area(), frame.buffer_mut());
    })
    .unwrap();

    term.backend_mut().set_size(Size::new(8, 2));
    term.draw(|frame| {
        assert_eq!(frame.area(), Rect::new(0, 0, 8, 2));
        Paragraph::new("xxxxx").render(frame.area(), frame.buffer_mut());
    })
    .unwrap();
    assert_eq!(term.size(), Size::new(8, 2));
}

#[test]
fn events_flow_through_the_stream() {
    let (term, tx) = harness(Size::new(4, 1));
    tx.send(Event::Key(KeyEvent::new(KeyCode::Char('z'))))
        .unwrap();
    let got = term
        .events()
        .poll(Some(Duration::from_millis(200)))
        .unwrap();
    assert_eq!(got, Some(Event::Key(KeyEvent::new(KeyCode::Char('z')))));
    // Nothing more queued: a short poll times out.
    assert_eq!(
        term.events().poll(Some(Duration::from_millis(20))).unwrap(),
        None
    );
}

#[test]
fn layout_composition_matches() {
    let rows = column([Constraint::Length(1), Constraint::Fill(1)]).split(Rect::new(0, 0, 10, 5));
    assert_eq!(rows[0], Rect::new(0, 0, 10, 1));
    assert_eq!(rows[1], Rect::new(0, 1, 10, 4));
}
