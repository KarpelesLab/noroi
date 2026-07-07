//! [`Paragraph`]: multi-line styled text with optional word wrapping.

use crate::buffer::{Buffer, char_width};
use crate::geom::Rect;
use crate::style::Style;
use crate::widget::block::Block;
use crate::widget::text::{Line, Span, Text};
use crate::widget::{Alignment, Widget, align_offset};
use alloc::string::String;
use alloc::vec::Vec;

/// Word-wrap configuration for a [`Paragraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wrap {
    /// When true, leading whitespace on wrapped continuation rows is trimmed.
    pub trim: bool,
}

/// A block of text rendered into an area, optionally framed by a [`Block`],
/// wrapped, aligned and vertically/horizontally scrolled.
#[derive(Debug, Clone, Default)]
pub struct Paragraph {
    text: Text,
    block: Option<Block>,
    wrap: Option<Wrap>,
    alignment: Option<Alignment>,
    scroll: (u16, u16),
}

impl Paragraph {
    /// Create a paragraph from anything convertible to [`Text`].
    pub fn new(text: impl Into<Text>) -> Self {
        Paragraph {
            text: text.into(),
            block: None,
            wrap: None,
            alignment: None,
            scroll: (0, 0),
        }
    }

    /// Wrap the paragraph in a [`Block`].
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Enable word wrapping.
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = Some(wrap);
        self
    }

    /// Force a horizontal alignment for every line (overrides per-line values).
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Scroll offset as `(rows_down, columns_right)`.
    pub fn scroll(mut self, offset: (u16, u16)) -> Self {
        self.scroll = offset;
        self
    }

    /// A base style patched beneath the text's own styles.
    pub fn style(mut self, style: Style) -> Self {
        self.text.style = style;
        self
    }

    /// The number of visual rows the text occupies at `width` (after wrapping).
    pub fn line_count(&self, width: u16) -> usize {
        self.visual_lines(width).len()
    }

    fn visual_lines(&self, width: u16) -> Vec<Line> {
        match self.wrap {
            Some(w) => {
                let mut out = Vec::new();
                for line in &self.text.lines {
                    let wrapped = wrap_line(line, width.max(1), w.trim);
                    out.extend(wrapped);
                }
                out
            }
            None => self.text.lines.clone(),
        }
    }
}

impl Widget for Paragraph {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let content_area = match &self.block {
            Some(block) => {
                block.render(area, buf);
                block.inner(area)
            }
            None => area,
        };
        if content_area.is_empty() {
            return;
        }

        let base = self.text.style;
        let lines = self.visual_lines(content_area.width);
        let (scroll_y, scroll_x) = self.scroll;

        let mut screen_y = content_area.y;
        for line in lines.iter().skip(scroll_y as usize) {
            if screen_y >= content_area.bottom() {
                break;
            }
            let alignment = self.alignment.unwrap_or(line.alignment);
            let line_width = line.width();
            let offset = align_offset(
                alignment,
                content_area.width,
                line_width.saturating_sub(scroll_x),
            );

            let mut x = content_area.x + offset;
            let mut col = 0u16; // display column within the unscrolled line
            'line: for span in &line.spans {
                let style = base.patch(span.style);
                for ch in span.content.chars() {
                    let cw = char_width(ch).max(1);
                    // Skip the first `scroll_x` display columns.
                    if col < scroll_x {
                        col += cw;
                        continue;
                    }
                    if x >= content_area.right() {
                        break 'line;
                    }
                    let adv = buf.set_char(x, screen_y, ch, style);
                    x = x.saturating_add(adv);
                    col += cw;
                }
            }
            screen_y += 1;
        }
    }
}

/// A run of characters — either all spaces or all non-spaces — sharing the
/// tokenized stream's styles.
struct Token {
    chars: Vec<(char, Style)>,
    is_space: bool,
    width: u16,
}

/// Wrap a single [`Line`] to `width`, preserving span styles. Uses greedy
/// word packing: whole words stay together where they fit, words longer than
/// `width` are hard-broken, and inter-word spaces are collapsed at wraps.
fn wrap_line(line: &Line, width: u16, trim: bool) -> Vec<Line> {
    let tokens = tokenize(line);
    if tokens.is_empty() {
        return alloc::vec![Line {
            spans: Vec::new(),
            alignment: line.alignment
        }];
    }

    let mut rows: Vec<Vec<(char, Style)>> = Vec::new();
    let mut cur: Vec<(char, Style)> = Vec::new();
    let mut cur_w: u16 = 0;

    for tok in tokens {
        if tok.is_space {
            if cur.is_empty() && (trim || !rows.is_empty()) {
                continue; // drop leading/continuation-row spaces
            }
            if cur_w + tok.width > width {
                // The gap runs off the edge: end the row and drop the spaces.
                rows.push(core::mem::take(&mut cur));
                cur_w = 0;
            } else {
                cur_w += tok.width;
                cur.extend(tok.chars);
            }
        } else if tok.width > width {
            // Hard-break a word wider than the whole line.
            for (c, style) in tok.chars {
                let cw = char_width(c).max(1);
                if cur_w + cw > width && !cur.is_empty() {
                    rows.push(core::mem::take(&mut cur));
                    cur_w = 0;
                }
                cur.push((c, style));
                cur_w += cw;
            }
        } else if cur_w + tok.width > width && !cur.is_empty() {
            trim_trailing_spaces(&mut cur, &mut cur_w);
            rows.push(core::mem::take(&mut cur));
            cur_w = tok.width;
            cur.extend(tok.chars);
        } else {
            cur_w += tok.width;
            cur.extend(tok.chars);
        }
    }
    trim_trailing_spaces(&mut cur, &mut cur_w);
    if !cur.is_empty() || rows.is_empty() {
        rows.push(cur);
    }

    rows.into_iter()
        .map(|row| Line {
            spans: coalesce(row),
            alignment: line.alignment,
        })
        .collect()
}

/// Split a line into alternating space / non-space [`Token`]s.
fn tokenize(line: &Line) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    for span in &line.spans {
        for c in span.content.chars() {
            let is_space = c == ' ';
            let cw = char_width(c).max(1);
            match tokens.last_mut() {
                Some(t) if t.is_space == is_space => {
                    t.chars.push((c, span.style));
                    t.width += cw;
                }
                _ => tokens.push(Token {
                    chars: alloc::vec![(c, span.style)],
                    is_space,
                    width: cw,
                }),
            }
        }
    }
    tokens
}

fn trim_trailing_spaces(cur: &mut Vec<(char, Style)>, cur_w: &mut u16) {
    while cur.last().map(|(c, _)| *c) == Some(' ') {
        cur.pop();
        *cur_w = cur_w.saturating_sub(1);
    }
}

/// Merge a `(char, style)` run into styled [`Span`]s, grouping equal styles.
fn coalesce(row: Vec<(char, Style)>) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (c, style) in row {
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.push(c),
            _ => spans.push(Span {
                content: String::from(c),
                style,
            }),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;

    #[test]
    fn wrap_breaks_on_spaces() {
        let line = Line::raw("the quick brown fox");
        let rows = wrap_line(&line, 9, false);
        let plain: Vec<String> = rows.iter().map(|l| l.to_plain()).collect();
        assert_eq!(
            plain,
            alloc::vec![String::from("the quick"), String::from("brown fox")]
        );
    }

    #[test]
    fn hard_break_long_word() {
        let line = Line::raw("abcdefghij");
        let rows = wrap_line(&line, 4, false);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].to_plain(), "abcd");
    }

    #[test]
    fn renders_into_buffer() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
        Paragraph::new("hello world")
            .wrap(Wrap { trim: false })
            .render(Rect::new(0, 0, 5, 2), &mut buf);
        assert_eq!(buf.get(0, 0).unwrap().symbol_char(), 'h');
    }
}
