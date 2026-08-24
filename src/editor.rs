use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::selection::CursorPosition;

#[derive(Debug, PartialEq, Eq)]
pub struct EditorBuffer {
    lines: Vec<String>,
    cursor: CursorPosition,
    dirty: bool,
}

impl EditorBuffer {
    pub fn from_text(text: &str) -> Self {
        let lines = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
        Self {
            lines: if lines.is_empty() {
                vec![String::new()]
            } else {
                lines
            },
            cursor: CursorPosition::new(0, 0),
            dirty: false,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn cursor(&self) -> CursorPosition {
        self.cursor
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn move_left(&mut self) {
        self.cursor.column = self.cursor.column.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let line_length = self.current_line_graphemes();
        self.cursor.column = self.cursor.column.saturating_add(1).min(line_length);
    }

    pub fn move_up(&mut self) {
        self.cursor.line = self.cursor.line.saturating_sub(1);
        self.clamp_column();
    }

    pub fn move_down(&mut self) {
        self.cursor.line = self
            .cursor
            .line
            .saturating_add(1)
            .min(self.lines.len().saturating_sub(1));
        self.clamp_column();
    }

    pub fn move_home(&mut self) {
        self.cursor.column = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor.column = self.current_line_graphemes();
    }

    pub fn insert(&mut self, character: char) {
        if character == '\n' {
            self.split_line();
            return;
        }

        let byte_index = self.current_byte_index();
        self.lines[self.cursor.line].insert(byte_index, character);
        self.cursor.column += 1;
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        if self.cursor.column > 0 {
            let line = &mut self.lines[self.cursor.line];
            let start = grapheme_byte_index(line, self.cursor.column - 1);
            let end = grapheme_byte_index(line, self.cursor.column);
            line.replace_range(start..end, "");
            self.cursor.column -= 1;
            self.dirty = true;
        } else if self.cursor.line > 0 {
            let current = self.lines.remove(self.cursor.line);
            self.cursor.line -= 1;
            self.cursor.column = self.current_line_graphemes();
            self.lines[self.cursor.line].push_str(&current);
            self.dirty = true;
        }
    }

    pub fn delete(&mut self) {
        let line = self.cursor.line;
        if self.cursor.column < self.current_line_graphemes() {
            let current = &mut self.lines[line];
            let start = grapheme_byte_index(current, self.cursor.column);
            let end = grapheme_byte_index(current, self.cursor.column + 1);
            current.replace_range(start..end, "");
            self.dirty = true;
        } else if line + 1 < self.lines.len() {
            let next = self.lines.remove(line + 1);
            self.lines[line].push_str(&next);
            self.dirty = true;
        }
    }

    fn split_line(&mut self) {
        let byte_index = self.current_byte_index();
        let remainder = self.lines[self.cursor.line].split_off(byte_index);
        self.lines.insert(self.cursor.line + 1, remainder);
        self.cursor.line += 1;
        self.cursor.column = 0;
        self.dirty = true;
    }

    fn current_line_graphemes(&self) -> usize {
        self.lines[self.cursor.line].graphemes(true).count()
    }

    fn current_byte_index(&self) -> usize {
        grapheme_byte_index(&self.lines[self.cursor.line], self.cursor.column)
    }

    fn clamp_column(&mut self) {
        self.cursor.column = self.cursor.column.min(self.current_line_graphemes());
    }

    pub fn cursor_display_column(&self) -> usize {
        let line = &self.lines[self.cursor.line];
        let byte_index = grapheme_byte_index(line, self.cursor.column);
        UnicodeWidthStr::width(&line[..byte_index])
    }
}

fn grapheme_byte_index(text: &str, grapheme_index: usize) -> usize {
    text.grapheme_indices(true)
        .nth(grapheme_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::EditorBuffer;
    use crate::selection::CursorPosition;

    #[test]
    fn edits_lines_and_preserves_newlines() {
        let mut editor = EditorBuffer::from_text("# Title\nbody");
        editor.move_end();
        editor.insert('!');
        editor.move_home();
        editor.insert('\n');

        assert_eq!(editor.text(), "\n# Title!\nbody");
        assert_eq!(editor.cursor(), CursorPosition::new(1, 0));
        assert!(editor.dirty());
    }

    #[test]
    fn backspace_joins_lines_at_the_start_of_a_line() {
        let mut editor = EditorBuffer::from_text("first\nsecond");
        editor.move_down();
        editor.backspace();

        assert_eq!(editor.text(), "firstsecond");
        assert_eq!(editor.cursor(), CursorPosition::new(0, 5));
    }

    #[test]
    fn delete_joins_lines_at_the_end_of_a_line() {
        let mut editor = EditorBuffer::from_text("first\nsecond");
        editor.move_end();
        editor.delete();

        assert_eq!(editor.text(), "firstsecond");
    }

    #[test]
    fn unicode_cursor_columns_use_terminal_width() {
        let mut editor = EditorBuffer::from_text("aé界");
        editor.move_right();
        editor.move_right();

        assert_eq!(editor.cursor_display_column(), 2);
    }
}
