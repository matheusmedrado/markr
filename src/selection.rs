use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CursorPosition {
    pub line: usize,
    pub column: usize,
}

impl CursorPosition {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: CursorPosition,
    pub head: CursorPosition,
}

impl Selection {
    pub fn new(anchor: CursorPosition, head: CursorPosition) -> Self {
        Self { anchor, head }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn normalized(&self) -> (CursorPosition, CursorPosition) {
        let start = self.min();
        let end = self.max();
        (start, end)
    }

    fn min(&self) -> CursorPosition {
        if self.anchor.line < self.head.line
            || (self.anchor.line == self.head.line && self.anchor.column <= self.head.column)
        {
            self.anchor
        } else {
            self.head
        }
    }

    fn max(&self) -> CursorPosition {
        if self.anchor.line > self.head.line
            || (self.anchor.line == self.head.line && self.anchor.column > self.head.column)
        {
            self.anchor
        } else {
            self.head
        }
    }

    pub fn text_with_margin(&self, lines: &[Line<'_>], content_margin: usize) -> String {
        let (raw_start, raw_end) = self.normalized();
        let Some(first) = clamp_position_with_margin(raw_start, lines, content_margin) else {
            return String::new();
        };
        let Some(second) = clamp_position_with_margin(raw_end, lines, content_margin) else {
            return String::new();
        };
        let (start, end) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };

        if start.line == end.line {
            let line = &lines[start.line];
            let text = content_text(line, content_margin);
            return slice_by_columns(&text, start.column, end.column);
        }

        let mut result = String::new();
        let first = &lines[start.line];
        let first_text = content_text(first, content_margin);
        result.push_str(&slice_by_columns(
            &first_text,
            start.column,
            text_width(&first_text),
        ));

        for line in &lines[start.line + 1..end.line] {
            result.push('\n');
            result.push_str(&content_text(line, content_margin));
        }

        if end.line < lines.len() {
            result.push('\n');
            let last = &lines[end.line];
            let last_text = content_text(last, content_margin);
            result.push_str(&slice_by_columns(&last_text, 0, end.column));
        }

        result
    }
}

pub fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

pub fn content_text(line: &Line<'_>, content_margin: usize) -> String {
    let text = line_text(line);
    slice_by_columns(&text, content_margin, text_width(&text))
}

pub fn text_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub fn slice_by_columns(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    let mut result = String::new();
    let mut column: usize = 0;
    for grapheme in text.graphemes(true) {
        let width = grapheme.width();
        let next_column = column.saturating_add(width);
        if width > 0 && column < end && next_column > start {
            result.push_str(grapheme);
        }
        column = next_column;
        if column >= end {
            break;
        }
    }
    result
}

pub fn clamp_position_with_margin(
    position: CursorPosition,
    lines: &[Line<'_>],
    content_margin: usize,
) -> Option<CursorPosition> {
    let line = position.line.min(lines.len().saturating_sub(1));
    lines.get(line).map(|line_content| {
        let text = content_text(line_content, content_margin);
        CursorPosition::new(line, position.column.min(text_width(&text)))
    })
}

pub fn selection_style(theme: Theme) -> Style {
    Style::default()
        .fg(theme.background)
        .bg(theme.accent)
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use ratatui::text::Line;

    use super::{CursorPosition, Selection};

    #[test]
    fn extracts_single_line_text() {
        let lines = vec![Line::from("hello world")];
        let selection = Selection::new(CursorPosition::new(0, 0), CursorPosition::new(0, 5));

        assert_eq!(selection.text_with_margin(&lines, 0), "hello");
    }

    #[test]
    fn extracts_multi_line_text() {
        let lines = vec![Line::from("first"), Line::from("second")];
        let selection = Selection::new(CursorPosition::new(0, 0), CursorPosition::new(1, 3));

        assert_eq!(selection.text_with_margin(&lines, 0), "first\nsec");
    }

    #[test]
    fn extracts_text_using_terminal_columns_for_unicode() {
        let lines = vec![Line::from("aé界b")];
        let selection = Selection::new(CursorPosition::new(0, 1), CursorPosition::new(0, 4));

        assert_eq!(selection.text_with_margin(&lines, 0), "é界");
    }

    #[test]
    fn ignores_the_reader_centering_margin_when_copying() {
        let lines = vec![Line::from("  hello")];
        let selection = Selection::new(CursorPosition::new(0, 0), CursorPosition::new(0, 5));

        assert_eq!(selection.text_with_margin(&lines, 2), "hello");
    }

    #[test]
    fn clamps_out_of_bounds_positions_without_panicking() {
        let lines = vec![Line::from("hello")];
        let selection = Selection::new(CursorPosition::new(0, 2), CursorPosition::new(99, 99));

        assert_eq!(selection.text_with_margin(&lines, 0), "llo");
    }

    #[test]
    fn empty_documents_have_no_selectable_position() {
        assert_eq!(
            super::clamp_position_with_margin(CursorPosition::new(0, 0), &[], 0),
            None
        );
    }
}
