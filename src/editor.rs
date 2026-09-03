use std::sync::atomic::{AtomicU64, Ordering};

use unicode_segmentation::UnicodeSegmentation;

use crate::selection::{CursorPosition, Selection};

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
    /// Where a selection was started, if one is in progress. The selection
    /// runs from here to the cursor, in either direction.
    anchor: Option<CursorPosition>,
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
            anchor: None,
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

    /// The selection in progress, if it covers anything at all. An anchor
    /// sitting on the cursor selects nothing, and is reported as nothing.
    pub fn selection(&self) -> Option<Selection> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| Selection::new(anchor, self.cursor))
    }

    /// Starts a selection here, unless one is already under way. Called
    /// before a movement that should extend one rather than replace it.
    pub fn begin_selection(&mut self) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    pub fn select_all(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        self.anchor = Some(CursorPosition::new(0, 0));
        self.cursor = CursorPosition::new(last, self.lines[last].graphemes(true).count());
        self.break_run();
    }

    /// The selected text, with the newlines it spans.
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_bounds()?;
        if start.line == end.line {
            return Some(
                grapheme_slice(&self.lines[start.line], start.column, end.column).to_owned(),
            );
        }

        let mut text = grapheme_slice(
            &self.lines[start.line],
            start.column,
            self.lines[start.line].graphemes(true).count(),
        )
        .to_owned();
        for line in &self.lines[start.line + 1..end.line] {
            text.push('\n');
            text.push_str(line);
        }
        text.push('\n');
        text.push_str(grapheme_slice(&self.lines[end.line], 0, end.column));
        Some(text)
    }

    /// Removes the selection, if there is one, as a single undo step.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_bounds() else {
            return false;
        };
        self.record_edit(EditRun::Delete, true);
        self.remove_range(start, end);
        self.anchor = None;
        self.run = None;
        true
    }

    /// Inserts `text` at the cursor, replacing the selection if there is one.
    /// The whole paste is one undo step, however many lines it carries.
    pub fn insert_str(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() {
            return;
        }

        self.record_edit(EditRun::Insert, true);
        if let Some((start, end)) = self.selection_bounds() {
            self.remove_range(start, end);
        }
        self.anchor = None;
        for character in text.chars() {
            self.write(character);
        }
        self.run = None;
    }

    /// The selection as an ordered pair, earliest position first.
    fn selection_bounds(&self) -> Option<(CursorPosition, CursorPosition)> {
        self.selection().map(|selection| selection.normalized())
    }

    /// Cuts the text between two positions out of the buffer and leaves the
    /// cursor where it began. Records nothing: the caller owns the undo step.
    fn remove_range(&mut self, start: CursorPosition, end: CursorPosition) {
        let tail = grapheme_slice(
            &self.lines[end.line],
            end.column,
            self.lines[end.line].graphemes(true).count(),
        )
        .to_owned();
        let head = grapheme_slice(&self.lines[start.line], 0, start.column).to_owned();

        self.lines.drain(start.line + 1..=end.line);
        self.lines[start.line] = head + &tail;
        self.cursor = start;
    }

    /// Writes one character at the cursor. Records nothing.
    fn write(&mut self, character: char) {
        if character == '\n' {
            self.split_line();
            return;
        }
        let byte_index = self.current_byte_index();
        self.lines[self.cursor.line].insert(byte_index, character);
        self.cursor.column += 1;
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
        // Typing over a selection replaces it, and both halves are one undo
        // step: taking back the letter should bring the replaced text back.
        if let Some((start, end)) = self.selection_bounds() {
            self.record_edit(EditRun::Insert, true);
            self.remove_range(start, end);
            self.anchor = None;
            self.write(character);
            self.run = None;
            return;
        }

        // A newline and the space that ends a word are the places a reader
        // expects one undo to stop, so neither joins the run around it.
        let boundary = character == '\n' || self.ends_a_word(character);
        self.record_edit(EditRun::Insert, boundary);
        self.write(character);
        self.run = (!boundary).then_some((EditRun::Insert, self.cursor));
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
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
        if self.delete_selection() {
            return;
        }
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
        self.anchor = None;
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

/// The text of `line` between two grapheme columns.
fn grapheme_slice(line: &str, start: usize, end: usize) -> &str {
    let from = grapheme_byte_index(line, start);
    let to = grapheme_byte_index(line, end);
    &line[from..to.max(from)]
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

    /// Selects from `from` to `to`.
    fn select(editor: &mut EditorBuffer, from: CursorPosition, to: CursorPosition) {
        editor.set_cursor(from);
        editor.begin_selection();
        editor.set_cursor(to);
    }

    #[test]
    fn an_anchor_on_the_cursor_selects_nothing() {
        let mut editor = EditorBuffer::from_text("hello");
        editor.begin_selection();

        assert!(editor.selection().is_none());
        assert!(editor.selected_text().is_none());
    }

    #[test]
    fn a_selection_reads_back_the_text_it_covers() {
        let mut editor = EditorBuffer::from_text("hello world");
        select(
            &mut editor,
            CursorPosition::new(0, 6),
            CursorPosition::new(0, 11),
        );

        assert_eq!(editor.selected_text().as_deref(), Some("world"));
    }

    #[test]
    fn a_selection_reads_back_across_lines_with_its_newlines() {
        let mut editor = EditorBuffer::from_text("first\nsecond\nthird");
        select(
            &mut editor,
            CursorPosition::new(0, 3),
            CursorPosition::new(2, 2),
        );

        assert_eq!(editor.selected_text().as_deref(), Some("st\nsecond\nth"));
    }

    #[test]
    fn a_backwards_selection_reads_the_same_as_a_forwards_one() {
        let mut editor = EditorBuffer::from_text("hello world");
        select(
            &mut editor,
            CursorPosition::new(0, 11),
            CursorPosition::new(0, 6),
        );

        assert_eq!(editor.selected_text().as_deref(), Some("world"));
    }

    #[test]
    fn typing_over_a_selection_replaces_it_in_one_undo_step() {
        let mut editor = EditorBuffer::from_text("hello world");
        select(
            &mut editor,
            CursorPosition::new(0, 6),
            CursorPosition::new(0, 11),
        );
        editor.insert('t');

        assert_eq!(editor.text(), "hello t");
        assert!(editor.selection().is_none(), "the selection is spent");

        editor.undo();
        assert_eq!(
            editor.text(),
            "hello world",
            "one undo brings back what was replaced"
        );
    }

    #[test]
    fn backspace_over_a_selection_removes_only_the_selection() {
        let mut editor = EditorBuffer::from_text("hello world");
        select(
            &mut editor,
            CursorPosition::new(0, 5),
            CursorPosition::new(0, 11),
        );
        editor.backspace();

        assert_eq!(editor.text(), "hello", "the character before it survives");
        assert_eq!(editor.cursor(), CursorPosition::new(0, 5));
    }

    #[test]
    fn deleting_a_selection_that_spans_lines_joins_what_is_left() {
        let mut editor = EditorBuffer::from_text("first\nsecond\nthird");
        select(
            &mut editor,
            CursorPosition::new(0, 2),
            CursorPosition::new(2, 2),
        );
        editor.delete();

        assert_eq!(editor.text(), "fiird");
        assert_eq!(editor.cursor(), CursorPosition::new(0, 2));
    }

    #[test]
    fn selecting_everything_covers_the_whole_buffer() {
        let mut editor = EditorBuffer::from_text("one\ntwo\nthree");
        editor.select_all();

        assert_eq!(editor.selected_text().as_deref(), Some("one\ntwo\nthree"));
    }

    #[test]
    fn pasting_replaces_the_selection_and_undoes_as_one_step() {
        let mut editor = EditorBuffer::from_text("hello world");
        select(
            &mut editor,
            CursorPosition::new(0, 6),
            CursorPosition::new(0, 11),
        );
        editor.insert_str("there\nfriend");

        assert_eq!(editor.text(), "hello there\nfriend");
        assert_eq!(editor.cursor(), CursorPosition::new(1, 6));

        editor.undo();
        assert_eq!(editor.text(), "hello world");
    }

    #[test]
    fn pasting_normalises_windows_line_endings() {
        let mut editor = EditorBuffer::from_text("");
        editor.insert_str("one\r\ntwo\rthree");

        assert_eq!(editor.text(), "one\ntwo\nthree");
    }

    #[test]
    fn undo_does_not_bring_a_selection_back_with_it() {
        let mut editor = EditorBuffer::from_text("hello world");
        select(
            &mut editor,
            CursorPosition::new(0, 6),
            CursorPosition::new(0, 11),
        );
        editor.insert('t');
        editor.undo();

        assert!(
            editor.selection().is_none(),
            "a restored buffer has a cursor, not a selection"
        );
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
