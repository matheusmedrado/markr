//! Soft wrapping for the editor.
//!
//! The reader reflows Markdown into the measure; the editor cannot, because
//! what it shows has to stay the file. So the editor keeps every source line
//! intact and instead lays each one across as many terminal rows as it needs.
//!
//! Everything here counts columns in graphemes, matching
//! [`CursorPosition::column`], and measures widths in terminal cells, so a
//! line of wide characters wraps where it actually runs out of room.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::selection::CursorPosition;

/// One terminal row of a soft-wrapped source line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualRow {
    /// The source line this row belongs to.
    pub line: usize,
    /// The grapheme columns of that line this row covers, half open. The rows
    /// of a line are contiguous and together cover all of it.
    pub start: usize,
    pub end: usize,
}

impl VisualRow {
    /// Whether this row opens its source line, and so carries the line number.
    /// Continuation rows leave the gutter blank.
    pub fn opens_line(self) -> bool {
        self.start == 0
    }
}

/// Where every source line falls on screen once wrapped, held against the
/// buffer revision and the width it was laid out for.
#[derive(Debug, Default)]
pub struct WrapLayout {
    rows: Vec<VisualRow>,
    /// Index into `rows` of the first row of each source line, so finding a
    /// cursor does not mean scanning from the top of the document.
    line_starts: Vec<usize>,
    built_from: Option<(u64, usize)>,
}

impl WrapLayout {
    /// Whether the held rows still describe `revision` at `width`.
    pub fn is_current(&self, revision: u64, width: usize) -> bool {
        self.built_from == Some((revision, width))
    }

    /// Re-wraps `lines`, which are expected to be revision `revision` of the
    /// editor buffer. Callers should check [`Self::is_current`] first.
    pub fn rebuild(&mut self, revision: u64, width: usize, lines: &[String]) {
        // A zero-width editor would wrap forever; one column always makes
        // progress, even if there is nothing useful to look at.
        let width = width.max(1);

        self.rows.clear();
        self.line_starts.clear();
        self.line_starts.reserve(lines.len());

        for (index, line) in lines.iter().enumerate() {
            self.line_starts.push(self.rows.len());
            wrap_line(line, width, |start, end| {
                self.rows.push(VisualRow {
                    line: index,
                    start,
                    end,
                });
            });
        }

        self.built_from = Some((revision, width));
    }

    pub fn rows(&self) -> &[VisualRow] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.line_starts.clear();
        self.built_from = None;
    }

    /// The row holding `cursor`, and how far across that row it sits in
    /// terminal cells.
    ///
    /// A cursor sitting exactly on a wrap point belongs to the row that
    /// follows it, so typing past the end of a row moves the caret to the
    /// start of the next one rather than leaving it off the right edge.
    pub fn locate(&self, cursor: CursorPosition, lines: &[String]) -> (usize, usize) {
        let Some(row) = self.row_of(cursor) else {
            return (0, 0);
        };
        let column = lines
            .get(self.rows[row].line)
            .map(|line| {
                let slice = grapheme_slice(line, self.rows[row].start, cursor.column);
                UnicodeWidthStr::width(slice)
            })
            .unwrap_or(0);
        (row, column)
    }

    /// The cursor position at `display_column` cells across visual row `row`.
    pub fn position_at(
        &self,
        row: usize,
        display_column: usize,
        lines: &[String],
    ) -> CursorPosition {
        let Some(row) = self.rows.get(row).copied() else {
            return CursorPosition::new(0, 0);
        };
        let Some(line) = lines.get(row.line) else {
            return CursorPosition::new(row.line, row.start);
        };

        let mut column = row.start;
        let mut width = 0usize;
        for grapheme in grapheme_slice(line, row.start, row.end).graphemes(true) {
            let next = width.saturating_add(UnicodeWidthStr::width(grapheme));
            if display_column < next {
                break;
            }
            width = next;
            column += 1;
        }
        CursorPosition::new(row.line, column)
    }

    /// Whether row `index` is the last row of its source line.
    pub fn ends_line(&self, index: usize) -> bool {
        match (self.rows.get(index), self.rows.get(index + 1)) {
            (Some(row), Some(next)) => row.line != next.line,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// The column just past the last thing actually printed on row `index`,
    /// ignoring the trailing space a wrap breaks after.
    pub fn last_printed_column(&self, index: usize, lines: &[String]) -> usize {
        let Some(row) = self.rows.get(index).copied() else {
            return 0;
        };
        let Some(line) = lines.get(row.line) else {
            return row.start;
        };

        let mut column = row.end;
        for grapheme in grapheme_slice(line, row.start, row.end)
            .graphemes(true)
            .rev()
        {
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            column -= 1;
        }
        column.max(row.start)
    }

    /// The index of the row holding `cursor`.
    fn row_of(&self, cursor: CursorPosition) -> Option<usize> {
        let first = *self.line_starts.get(cursor.line)?;
        let last = self
            .line_starts
            .get(cursor.line + 1)
            .copied()
            .unwrap_or(self.rows.len())
            .checked_sub(1)?;

        for index in first..=last {
            if cursor.column < self.rows[index].end {
                return Some(index);
            }
        }
        Some(last)
    }
}

/// Splits one source line into the grapheme ranges that each fit `width`
/// cells, preferring to break after a space so words stay whole.
fn wrap_line(line: &str, width: usize, mut emit: impl FnMut(usize, usize)) {
    let graphemes: Vec<&str> = line.graphemes(true).collect();
    if graphemes.is_empty() {
        // An empty line still occupies a row, and still needs a caret.
        emit(0, 0);
        return;
    }

    let mut start = 0usize;
    let mut used = 0usize;
    // The column just past the most recent space in the row being built, which
    // is where the line breaks if the row fills up.
    let mut break_at: Option<usize> = None;

    for (column, grapheme) in graphemes.iter().enumerate() {
        let cell_width = UnicodeWidthStr::width(*grapheme);

        if used + cell_width > width && column > start {
            // Break after the last space if there was one, otherwise mid-word:
            // a run longer than the measure still has to go somewhere.
            let end = break_at.filter(|at| *at > start).unwrap_or(column);
            emit(start, end);
            start = end;
            used = grapheme_width(&graphemes[start..column]);
            break_at = None;
        }

        used += cell_width;
        if grapheme.chars().all(char::is_whitespace) {
            break_at = Some(column + 1);
        }
    }

    emit(start, graphemes.len());
}

fn grapheme_width(graphemes: &[&str]) -> usize {
    graphemes
        .iter()
        .map(|grapheme| UnicodeWidthStr::width(*grapheme))
        .sum()
}

/// The text of `line` between two grapheme columns.
fn grapheme_slice(line: &str, start: usize, end: usize) -> &str {
    let from = byte_index(line, start);
    let to = byte_index(line, end);
    &line[from..to.max(from)]
}

fn byte_index(line: &str, column: usize) -> usize {
    line.grapheme_indices(true)
        .nth(column)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

#[cfg(test)]
mod tests {
    use super::WrapLayout;
    use crate::selection::CursorPosition;

    fn layout(lines: &[&str], width: usize) -> (WrapLayout, Vec<String>) {
        let lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
        let mut wrap = WrapLayout::default();
        wrap.rebuild(1, width, &lines);
        (wrap, lines)
    }

    /// The text each visual row would draw.
    fn rendered(wrap: &WrapLayout, lines: &[String]) -> Vec<String> {
        wrap.rows()
            .iter()
            .map(|row| super::grapheme_slice(&lines[row.line], row.start, row.end).to_owned())
            .collect()
    }

    #[test]
    fn a_short_line_occupies_one_row() {
        let (wrap, lines) = layout(&["short"], 20);

        assert_eq!(wrap.len(), 1);
        assert_eq!(rendered(&wrap, &lines), vec!["short"]);
    }

    #[test]
    fn an_empty_line_still_occupies_a_row() {
        let (wrap, lines) = layout(&["", "after"], 20);

        assert_eq!(wrap.len(), 2);
        assert_eq!(rendered(&wrap, &lines), vec!["", "after"]);
    }

    #[test]
    fn wrapping_breaks_after_a_space_so_words_stay_whole() {
        let (wrap, lines) = layout(&["the quick brown fox"], 10);

        // Every row fits, and no word is split across two of them.
        assert_eq!(rendered(&wrap, &lines), vec!["the quick ", "brown fox"]);
    }

    #[test]
    fn a_word_longer_than_the_measure_breaks_mid_word() {
        let (wrap, lines) = layout(&["supercalifragilistic"], 8);

        assert_eq!(
            rendered(&wrap, &lines),
            vec!["supercal", "ifragili", "stic"],
            "a run with nowhere to break still has to go somewhere"
        );
    }

    #[test]
    fn rows_of_a_line_cover_it_exactly_once() {
        let (wrap, lines) = layout(&["the quick brown fox jumps over it"], 9);

        let mut expected = 0;
        for row in wrap.rows() {
            assert_eq!(row.start, expected, "rows must be contiguous");
            expected = row.end;
        }
        assert_eq!(
            expected,
            lines[0].chars().count(),
            "rows must cover the line"
        );
    }

    #[test]
    fn only_the_first_row_of_a_line_carries_the_number() {
        let (wrap, _) = layout(&["the quick brown fox"], 10);

        let opens: Vec<bool> = wrap.rows().iter().map(|row| row.opens_line()).collect();
        assert_eq!(opens, vec![true, false]);
    }

    #[test]
    fn wide_graphemes_wrap_by_the_room_they_take() {
        // Each of these is two cells wide, so only three fit in six columns.
        let (wrap, lines) = layout(&["界界界界"], 6);

        assert_eq!(rendered(&wrap, &lines), vec!["界界界", "界"]);
    }

    #[test]
    fn locating_a_cursor_gives_its_row_and_cell_offset() {
        let (wrap, lines) = layout(&["the quick brown fox"], 10);

        // Start of the second row: "brown fox" begins at column 10.
        assert_eq!(wrap.locate(CursorPosition::new(0, 10), &lines), (1, 0));
        // Two graphemes into that row.
        assert_eq!(wrap.locate(CursorPosition::new(0, 12), &lines), (1, 2));
        // Start of the document.
        assert_eq!(wrap.locate(CursorPosition::new(0, 0), &lines), (0, 0));
    }

    #[test]
    fn a_cursor_on_a_wrap_point_belongs_to_the_row_that_follows() {
        let (wrap, lines) = layout(&["the quick brown fox"], 10);

        // Column 10 ends row 0 and starts row 1; the caret goes to row 1, so
        // typing past the edge does not park it off the right of the screen.
        assert_eq!(wrap.locate(CursorPosition::new(0, 10), &lines).0, 1);
        // The very end of the line stays on the last row.
        let end = lines[0].chars().count();
        assert_eq!(wrap.locate(CursorPosition::new(0, end), &lines).0, 1);
    }

    #[test]
    fn a_position_round_trips_through_a_row_and_back() {
        let (wrap, lines) = layout(&["the quick brown fox", "second line"], 10);

        for line in 0..lines.len() {
            for column in 0..=lines[line].chars().count() {
                let cursor = CursorPosition::new(line, column);
                let (row, display) = wrap.locate(cursor, &lines);
                assert_eq!(
                    wrap.position_at(row, display, &lines),
                    cursor,
                    "cursor {cursor:?} did not survive the round trip"
                );
            }
        }
    }

    #[test]
    fn clicking_past_the_end_of_a_row_lands_on_its_last_column() {
        let (wrap, lines) = layout(&["ab", "cd"], 10);

        assert_eq!(
            wrap.position_at(0, 99, &lines),
            CursorPosition::new(0, 2),
            "a click in the empty space after a line goes to its end"
        );
    }

    #[test]
    fn clicking_inside_a_wide_grapheme_stays_on_a_boundary() {
        let (wrap, lines) = layout(&["a界b"], 10);

        assert_eq!(wrap.position_at(0, 1, &lines), CursorPosition::new(0, 1));
        // The second cell of a two-cell grapheme still selects that grapheme.
        assert_eq!(wrap.position_at(0, 2, &lines), CursorPosition::new(0, 1));
        assert_eq!(wrap.position_at(0, 3, &lines), CursorPosition::new(0, 2));
    }

    #[test]
    fn rebuilding_is_skipped_until_the_revision_or_the_width_moves() {
        let (wrap, _) = layout(&["some text"], 10);

        assert!(wrap.is_current(1, 10));
        assert!(!wrap.is_current(2, 10), "an edit invalidates the wrap");
        assert!(!wrap.is_current(1, 20), "a resize invalidates the wrap");
    }

    #[test]
    fn every_line_is_reachable_after_a_rebuild() {
        let (wrap, lines) = layout(&["one", "a much longer second line here", "three"], 12);

        for (index, line) in lines.iter().enumerate() {
            let cursor = CursorPosition::new(index, line.chars().count());
            let (row, _) = wrap.locate(cursor, &lines);
            assert_eq!(wrap.rows()[row].line, index);
        }
    }
}
