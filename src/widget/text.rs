//! Styled-text primitives: [`Span`], [`Line`] and [`Text`].
//!
//! These compose the way you would expect: a [`Span`] is a run of characters
//! sharing one [`Style`]; a [`Line`] is a sequence of spans on one row; a
//! [`Text`] is a stack of lines. [`Paragraph`](super::Paragraph) renders a
//! [`Text`], but the primitives are also useful on their own for titles, list
//! items and labels.

use crate::buffer::char_width;
use crate::style::Style;
use crate::widget::Alignment;
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// A run of text sharing a single [`Style`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    /// The text content (no newlines expected).
    pub content: String,
    /// The style applied to every cell of `content`.
    pub style: Style,
}

impl Span {
    /// A span with the default (empty) style.
    pub fn raw(content: impl Into<String>) -> Self {
        Span {
            content: content.into(),
            style: Style::RESET,
        }
    }

    /// A span with an explicit style.
    pub fn styled(content: impl Into<String>, style: Style) -> Self {
        Span {
            content: content.into(),
            style,
        }
    }

    /// The display width of the span in cells.
    pub fn width(&self) -> u16 {
        self.content.chars().map(char_width).sum()
    }
}

impl From<&str> for Span {
    fn from(s: &str) -> Self {
        Span::raw(s)
    }
}

impl From<String> for Span {
    fn from(s: String) -> Self {
        Span::raw(s)
    }
}

/// A single row of [`Span`]s with a horizontal [`Alignment`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    /// The spans, left to right.
    pub spans: Vec<Span>,
    /// How the line is aligned within its area.
    pub alignment: Alignment,
}

impl Line {
    /// A line from a single unstyled string.
    pub fn raw(content: impl Into<String>) -> Self {
        Line {
            spans: vec![Span::raw(content)],
            alignment: Alignment::Left,
        }
    }

    /// A line from a single styled string.
    pub fn styled(content: impl Into<String>, style: Style) -> Self {
        Line {
            spans: vec![Span::styled(content, style)],
            alignment: Alignment::Left,
        }
    }

    /// A line from a list of spans.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Line {
            spans: spans.into_iter().collect(),
            alignment: Alignment::Left,
        }
    }

    /// Set the alignment (builder style).
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Center this line.
    pub fn centered(self) -> Self {
        self.alignment(Alignment::Center)
    }

    /// Right-align this line.
    pub fn right_aligned(self) -> Self {
        self.alignment(Alignment::Right)
    }

    /// The total display width of all spans.
    pub fn width(&self) -> u16 {
        self.spans.iter().map(Span::width).sum()
    }

    /// The line's characters as one owned string (styles dropped).
    pub fn to_plain(&self) -> String {
        let mut s = String::new();
        for span in &self.spans {
            s.push_str(&span.content);
        }
        s
    }
}

impl From<&str> for Line {
    fn from(s: &str) -> Self {
        Line::raw(s)
    }
}

impl From<String> for Line {
    fn from(s: String) -> Self {
        Line::raw(s)
    }
}

impl From<Span> for Line {
    fn from(span: Span) -> Self {
        Line {
            spans: vec![span],
            alignment: Alignment::Left,
        }
    }
}

/// A block of text: a vertical stack of [`Line`]s plus a base [`Style`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Text {
    /// The lines, top to bottom.
    pub lines: Vec<Line>,
    /// A base style patched under every span's own style.
    pub style: Style,
}

impl Text {
    /// Build a [`Text`] by splitting `content` on newlines.
    pub fn raw(content: impl Into<Cow<'static, str>>) -> Self {
        let content = content.into();
        let lines = content.split('\n').map(Line::raw).collect();
        Text {
            lines,
            style: Style::RESET,
        }
    }

    /// Build a [`Text`] from explicit lines.
    pub fn from_lines(lines: impl IntoIterator<Item = Line>) -> Self {
        Text {
            lines: lines.into_iter().collect(),
            style: Style::RESET,
        }
    }

    /// Set the base style patched beneath every span.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The width of the widest line.
    pub fn width(&self) -> u16 {
        self.lines.iter().map(Line::width).max().unwrap_or(0)
    }

    /// The number of lines.
    pub fn height(&self) -> u16 {
        self.lines.len() as u16
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        let lines = s.split('\n').map(Line::raw).collect();
        Text {
            lines,
            style: Style::RESET,
        }
    }
}

impl From<String> for Text {
    fn from(s: String) -> Self {
        Text::from(s.as_str())
    }
}

impl From<Line> for Text {
    fn from(line: Line) -> Self {
        Text {
            lines: vec![line],
            style: Style::RESET,
        }
    }
}
