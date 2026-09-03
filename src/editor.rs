use std::sync::atomic::{AtomicU64, Ordering};

use unicode_segmentation::UnicodeSegmentation;

use crate::selection::CursorPosition;

#[derive(Debug, PartialEq, Eq)]
pub struct EditorBuffer {
    lines: Vec<String>,
    cursor: CursorPosition,
    saved_text: String,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    dirty: bool,
    /// Where this buffer stands in [`NEXT_REVISION`]. A cache built from the
    /// buffer can compare revisions to tell whether the text it holds is
    /// still current, without comparing the text itself.
    revision: u64,
    /// The run of same-shaped edits in progress, and the cursor it left
    /// behind. Typing a word records one undo snapshot rather than one per
    /// character; the run ends when the shape changes, the cursor moves
    /// somewhere else, or the word does.
    run: Option<(EditRun, CursorPosition)>,
}

/// The shape of an edit, for deciding whether it extends the current run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditRun {
    Insert,
    Backspace,
    Delete,
}

/// Revisions are drawn from one counter shared by every buffer, so no two
/// states of any two buffers ever share a number. Counting per buffer instead
/// would let a freshly loaded buffer open on the same revision as the one it
/// replaced — and a cache would then keep showing the file that was there
/// before, which is what reloading was meant to fix.
static NEXT_REVISION: AtomicU64 = AtomicU64::new(0);

fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditorSnapshot {
    lines: Vec<String>,
    cursor: CursorPosition,
}

impl EditorBuffer {
    pub fn from_text(text: &str) -> Self {
        let lines = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        Self {
            saved_text: lines.join("\n"),
            lines,
            cursor: CursorPosition::new(0, 0),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            dirty: false,
            revision: next_revision(),
            run: None,
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

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn mark_clean(&mut self) {
        self.saved_text = self.text();
        self.dirty = false;
    }

    pub fn set_cursor(&mut self, cursor: CursorPosition) {
        self.break_run();
        self.cursor.line = cursor.line.min(self.lines.len().saturating_sub(1));
        self.cursor.column = cursor.column.min(self.current_line_graphemes());
    }

    pub fn undo(&mut self) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(self.snapshot());
        self.restore(previous);
    }

    pub fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(self.snapshot());
        self.restore(next);
    }

    pub fn move_left(&mut self) {
        self.break_run();
        self.cursor.column = self.cursor.column.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.break_run();
        let line_length = self.current_line_graphemes();
        self.cursor.column = self.cursor.column.saturating_add(1).min(line_length);
    }

    pub fn insert(&mut self, character: char) {
        // A newline and the space that ends a word are the places a reader
        // expects one undo to stop, so neither joins the run around it.
        let boundary = character == '\n' || self.ends_a_word(character);
        self.record_edit(EditRun::Insert, boundary);

        if character == '\n' {
            self.split_line();
        } else {
            let byte_index = self.current_byte_index();
            self.lines[self.cursor.line].insert(byte_index, character);
            self.cursor.column += 1;
        }

        self.run = (!boundary).then_some((EditRun::Insert, self.cursor));
    }

    pub fn backspace(&mut self) {
        if self.cursor.column > 0 {
            self.record_edit(EditRun::Backspace, false);
            let line = &mut self.lines[self.cursor.line];
            let start = grapheme_byte_index(line, self.cursor.column - 1);
            let end = grapheme_byte_index(line, self.cursor.column);
            line.replace_range(start..end, "");
            self.cursor.column -= 1;
            self.run = Some((EditRun::Backspace, self.cursor));
        } else if self.cursor.line > 0 {
            // Joining two lines is a structural change rather than more of
            // the same deletion, so it stands as its own undo step.
            self.record_edit(EditRun::Backspace, true);
            let current = self.lines.remove(self.cursor.line);
            self.cursor.line -= 1;
            self.cursor.column = self.current_line_graphemes();
            self.lines[self.cursor.line].push_str(&current);
            self.run = None;
        }
    }

    pub fn delete(&mut self) {
        let line = self.cursor.line;
        if self.cursor.column < self.current_line_graphemes() {
            self.record_edit(EditRun::Delete, false);
            let current = &mut self.lines[line];
            let start = grapheme_byte_index(current, self.cursor.column);
            let end = grapheme_byte_index(current, self.cursor.column + 1);
            current.replace_range(start..end, "");
            self.run = Some((EditRun::Delete, self.cursor));
        } else if line + 1 < self.lines.len() {
            self.record_edit(EditRun::Delete, true);
            let next = self.lines.remove(line + 1);
            self.lines[line].push_str(&next);
            self.run = None;
        }
    }

    fn split_line(&mut self) {
        let byte_index = self.current_byte_index();
        let remainder = self.lines[self.cursor.line].split_off(byte_index);
        self.lines.insert(self.cursor.line + 1, remainder);
        self.cursor.line += 1;
        self.cursor.column = 0;
    }

    fn current_line_graphemes(&self) -> usize {
        self.lines[self.cursor.line].graphemes(true).count()
    }

    fn current_byte_index(&self) -> usize {
        grapheme_byte_index(&self.lines[self.cursor.line], self.cursor.column)
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        }
    }

    fn restore(&mut self, snapshot: EditorSnapshot) {
        self.lines = snapshot.lines;
        self.cursor = snapshot.cursor;
        self.dirty = self.text() != self.saved_text;
        self.revision = next_revision();
        self.break_run();
    }

    /// Notes that the buffer is about to change, and records an undo
    /// snapshot unless this edit merely extends the run already in progress.
    ///
    /// `self.cursor` is still where the previous edit left it, so an edit
    /// continues a run when it has the same shape and starts exactly where
    /// that edit finished. Moving the cursor anywhere else ends the run.
    fn record_edit(&mut self, kind: EditRun, boundary: bool) {
        self.revision = next_revision();
        self.dirty = true;
        self.redo_stack.clear();

        if boundary || self.run != Some((kind, self.cursor)) {
            self.undo_stack.push(self.snapshot());
        }
    }

    /// Ends the current run, so the next edit starts a fresh undo step.
    fn break_run(&mut self) {
        self.run = None;
    }

    /// Whether inserting `character` here closes a word, which is where one
    /// undo should stop rather than swallowing the whole sentence.
    fn ends_a_word(&self, character: char) -> bool {
        if !character.is_whitespace() {
            return false;
        }
        let line = &self.lines[self.cursor.line];
        let before = grapheme_byte_index(line, self.cursor.column);
        line[..before]
            .chars()
            .next_back()
            .is_some_and(|previous| !previous.is_whitespace())
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

    /// `set_cursor` clamps to the line, so this parks the cursor at its end.
    fn move_to_end_of_line(editor: &mut EditorBuffer, line: usize) {
        editor.set_cursor(CursorPosition::new(line, usize::MAX));
    }

    #[test]
    fn edits_lines_and_preserves_newlines() {
        let mut editor = EditorBuffer::from_text("# Title\nbody");
        move_to_end_of_line(&mut editor, 0);
        editor.insert('!');
        editor.set_cursor(CursorPosition::new(0, 0));
        editor.insert('\n');

        assert_eq!(editor.text(), "\n# Title!\nbody");
        assert_eq!(editor.cursor(), CursorPosition::new(1, 0));
        assert!(editor.dirty());
    }

    #[test]
    fn backspace_joins_lines_at_the_start_of_a_line() {
        let mut editor = EditorBuffer::from_text("first\nsecond");
        editor.set_cursor(CursorPosition::new(1, 0));
        editor.backspace();

        assert_eq!(editor.text(), "firstsecond");
        assert_eq!(editor.cursor(), CursorPosition::new(0, 5));
    }

    #[test]
    fn delete_joins_lines_at_the_end_of_a_line() {
        let mut editor = EditorBuffer::from_text("first\nsecond");
        move_to_end_of_line(&mut editor, 0);
        editor.delete();

        assert_eq!(editor.text(), "firstsecond");
    }

    #[test]
    fn undoes_and_redoes_text_edits_and_restores_the_dirty_state() {
        let mut editor = EditorBuffer::from_text("hello");
        move_to_end_of_line(&mut editor, 0);
        editor.insert('!');
        assert_eq!(editor.text(), "hello!");
        assert!(editor.dirty());

        editor.undo();
        assert_eq!(editor.text(), "hello");
        assert!(!editor.dirty());

        editor.redo();
        assert_eq!(editor.text(), "hello!");
        assert!(editor.dirty());
    }

    #[test]
    fn typing_a_word_undoes_as_one_step() {
        let mut editor = EditorBuffer::from_text("");
        for character in "hello".chars() {
            editor.insert(character);
        }
        assert_eq!(editor.text(), "hello");

        editor.undo();
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn each_word_is_its_own_undo_step() {
        let mut editor = EditorBuffer::from_text("");
        for character in "hello world".chars() {
            editor.insert(character);
        }

        // The space closes "hello", so it travels with the word it follows.
        editor.undo();
        assert_eq!(editor.text(), "hello ");
        editor.undo();
        assert_eq!(editor.text(), "hello");
        editor.undo();
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn a_newline_is_its_own_undo_step() {
        let mut editor = EditorBuffer::from_text("");
        for character in "one\ntwo".chars() {
            editor.insert(character);
        }

        editor.undo();
        assert_eq!(editor.text(), "one\n");
        editor.undo();
        assert_eq!(editor.text(), "one");
    }

    #[test]
    fn moving_the_cursor_ends_the_run() {
        let mut editor = EditorBuffer::from_text("ab");
        move_to_end_of_line(&mut editor, 0);
        editor.insert('c');
        editor.set_cursor(CursorPosition::new(0, 0));
        editor.insert('z');

        assert_eq!(editor.text(), "zabc");
        editor.undo();
        assert_eq!(editor.text(), "abc");
        editor.undo();
        assert_eq!(editor.text(), "ab");
    }

    #[test]
    fn deleting_does_not_join_the_run_of_typing_before_it() {
        let mut editor = EditorBuffer::from_text("");
        for character in "abc".chars() {
            editor.insert(character);
        }
        editor.backspace();
        assert_eq!(editor.text(), "ab");

        editor.undo();
        assert_eq!(editor.text(), "abc");
        editor.undo();
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn a_run_of_backspaces_undoes_as_one_step() {
        let mut editor = EditorBuffer::from_text("hello");
        move_to_end_of_line(&mut editor, 0);
        editor.backspace();
        editor.backspace();
        assert_eq!(editor.text(), "hel");

        editor.undo();
        assert_eq!(editor.text(), "hello");
    }

    #[test]
    fn two_buffers_never_share_a_revision() {
        // A reload replaces the buffer; if the new one could open on the
        // revision the old one held, a cache would keep the old file on
        // screen. See `NEXT_REVISION`.
        let first = EditorBuffer::from_text("# One");
        let second = EditorBuffer::from_text("# One");

        assert_ne!(first.revision(), second.revision());
    }

    #[test]
    fn the_revision_moves_with_every_change_and_stands_still_otherwise() {
        let mut editor = EditorBuffer::from_text("hi");
        let start = editor.revision();

        move_to_end_of_line(&mut editor, 0);
        assert_eq!(
            editor.revision(),
            start,
            "moving the cursor is not a change"
        );

        editor.insert('!');
        let typed = editor.revision();
        assert_ne!(typed, start);

        // Coalesced edits still move the revision: the text changed, even
        // though no new undo step was recorded.
        editor.insert('?');
        assert_ne!(editor.revision(), typed);

        let before_undo = editor.revision();
        editor.undo();
        assert_ne!(editor.revision(), before_undo);
    }
}
