use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::images::{Asset, ImageStore};
use crate::markdown::{Block, Document, Inline, InlineStyle};
use crate::syntax;
use crate::theme::Theme;

#[derive(Debug)]
pub struct DocumentLayout {
    pub lines: Vec<Line<'static>>,
    pub heading_lines: Vec<usize>,
    pub image_regions: Vec<ImageRegion>,
    pub content_margin: usize,
}

#[derive(Debug)]
pub struct ImageRegion {
    pub line: usize,
    pub src: String,
}

impl DocumentLayout {
    pub fn heading_line(&self, heading_index: usize) -> Option<usize> {
        self.heading_lines.get(heading_index).copied()
    }
}

/// The ceiling on the measure. The reader otherwise fills whatever the terminal
/// gives it; this is a guard rail for an ultrawide window, set high enough that
/// an ordinary maximised terminal never reaches it.
pub const MAX_MEASURE: usize = 200;

/// Columns left for the measure and its centring once the gutter and the right
/// pad are taken out.
fn available_width(width: u16) -> usize {
    usize::from(width.max(1))
        .saturating_sub(GUTTER_WIDTH.saturating_add(RIGHT_PAD))
        .max(1)
}

/// The reading measure for a reader `width` columns wide.
///
/// The reader takes every column it is given, less the gutter and the right
/// pad, up to [`MAX_MEASURE`]. Growing more slowly than the terminal would be
/// kinder to the eye, but it leaves a window's worth of empty margin on a wide
/// terminal, which is not what a terminal reader is for.
pub fn measure_for(width: u16) -> usize {
    available_width(width).min(MAX_MEASURE)
}

/// Columns reserved at the left of every reader line: two of padding, the
/// heading-level tick, then one space before the text begins.
pub const GUTTER_WIDTH: usize = 4;

/// Columns kept clear at the right of the measure.
pub const RIGHT_PAD: usize = 2;

/// The reading-progress rail owns the reader's last column.
pub const RAIL_WIDTH: u16 = 1;

/// The reader's text area. The reader has no frame, so this is the whole area
/// minus the progress rail; the left gutter and right pad live inside the laid
/// out lines and are accounted for by [`DocumentLayout::content_margin`].
pub fn reader_inner(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(RAIL_WIDTH),
        area.height,
    )
}

/// The editor's text area. Its line-number gutter stands in for the rail, so it
/// uses the full area.
pub fn editor_inner(area: Rect) -> Rect {
    area
}

pub fn build(document: &Document, width: u16, theme: Theme, images: &ImageStore) -> DocumentLayout {
    let available = available_width(width);
    let max_width = measure_for(width);
    let mut lines = Vec::new();
    let mut heading_lines = Vec::new();
    let mut heading_levels = Vec::new();
    let mut image_regions = Vec::new();

    for block in &document.blocks {
        match block {
            Block::Heading { level, content } => {
                if !lines.is_empty() {
                    lines.push(Line::default());
                    if *level <= 2 {
                        lines.push(Line::default());
                    }
                }
                heading_lines.push(lines.len());
                heading_levels.push(*level);
                push_wrapped_inlines(
                    content,
                    None,
                    None,
                    heading_style(*level, theme),
                    max_width,
                    theme,
                    &mut lines,
                );
                // H1 and H2 close with the same hairline rule, as they do on
                // the web. H1 is set apart by its colour, not by a louder rule.
                if matches!(*level, 1 | 2) {
                    lines.push(Line::from(Span::styled(
                        "─".repeat(max_width),
                        Style::default().fg(theme.reader_border),
                    )));
                }
            }
            Block::Paragraph {
                content,
                quote_depth,
            } => {
                let (prefix, base_style) = if *quote_depth > 0 {
                    (
                        Some(Prefix {
                            text: "▎  ".repeat(*quote_depth as usize),
                            style: Style::default().fg(theme.accent_soft),
                        }),
                        Style::default()
                            .fg(theme.text_muted)
                            .add_modifier(Modifier::ITALIC),
                    )
                } else {
                    (None, Style::default().fg(theme.text))
                };
                let continuation = prefix.as_ref().map(|prefix| prefix.text.clone());
                push_wrapped_inlines(
                    content,
                    prefix,
                    continuation,
                    base_style,
                    max_width,
                    theme,
                    &mut lines,
                );
            }
            Block::List { ordered, items } => {
                for (index, item) in items.iter().enumerate() {
                    let marker = ordered
                        .map(|start| format!("{}.", start + index as u64))
                        .unwrap_or_else(|| "•".to_string());
                    let prefix = Prefix {
                        text: format!("{marker}  "),
                        style: Style::default().fg(theme.marker),
                    };
                    let continuation = " ".repeat(text_width(prefix.text.as_str()));
                    push_wrapped_inlines(
                        item,
                        Some(prefix),
                        Some(continuation),
                        Style::default().fg(theme.text),
                        max_width,
                        theme,
                        &mut lines,
                    );
                }
            }
            Block::FencedCode { language, code } => {
                render_code_block(language.as_deref(), code, max_width, theme, &mut lines);
            }
            Block::Table { headers, rows } => {
                render_table(headers, rows, max_width, theme, &mut lines);
            }
            Block::ThematicBreak => lines.push(Line::from(Span::styled(
                "┄".repeat(max_width),
                Style::default().fg(theme.reader_border),
            ))),
            Block::Image { src, alt } => match images.asset(src) {
                Some(Asset::Ready { rows, .. }) => {
                    image_regions.push(ImageRegion {
                        line: lines.len(),
                        src: src.clone(),
                    });
                    for _ in 0..*rows {
                        lines.push(Line::default());
                    }
                }
                Some(Asset::Missing) | None => {
                    render_image_placeholder(alt, src, theme, max_width, &mut lines)
                }
            },
        }
        lines.push(Line::default());
    }

    let margin = available.saturating_sub(max_width) / 2;
    let mut ticks = vec![None; lines.len()];
    for (line, level) in heading_lines.iter().zip(heading_levels.iter()) {
        if let Some(slot) = ticks.get_mut(*line) {
            *slot = Some(*level);
        }
    }

    let indent = " ".repeat(margin);
    for (index, line) in lines.iter_mut().enumerate() {
        let tick = match ticks.get(index).copied().flatten() {
            // A rule already marks H1 and H2; a tick as well marks them twice.
            Some(level) if level >= 3 => {
                Span::styled("▌", Style::default().fg(heading_tick(level, theme)))
            }
            _ => Span::raw(" "),
        };
        line.spans.insert(0, Span::raw(" "));
        line.spans.insert(0, tick);
        line.spans.insert(0, Span::raw("  "));
        if margin > 0 {
            line.spans.insert(0, Span::raw(indent.clone()));
        }
    }

    DocumentLayout {
        lines,
        heading_lines,
        image_regions,
        content_margin: margin.saturating_add(GUTTER_WIDTH),
    }
}

struct Prefix {
    text: String,
    style: Style,
}

fn push_wrapped_inlines(
    content: &[Inline],
    prefix: Option<Prefix>,
    continuation_prefix: Option<String>,
    base_style: Style,
    max_width: usize,
    theme: Theme,
    lines: &mut Vec<Line<'static>>,
) {
    let mut builder = LineBuilder::new(prefix, continuation_prefix, max_width);
    for inline in content {
        builder.push_text(
            &inline.text,
            inline_style(inline.style, inline.link.is_some(), base_style, theme),
            lines,
        );
    }
    builder.finish(lines);
}

fn render_image_placeholder(
    alt: &str,
    src: &str,
    theme: Theme,
    max_width: usize,
    lines: &mut Vec<Line<'static>>,
) {
    let bar = Style::default().fg(theme.accent_soft).bg(theme.surface);
    let slab = Style::default().bg(theme.surface);
    let inner_width = max_width.saturating_sub(3);
    let title = if alt.is_empty() { src } else { alt };

    for (text, style) in [
        (
            title,
            Style::default().fg(theme.text_muted).bg(theme.surface),
        ),
        (src, Style::default().fg(theme.border).bg(theme.surface)),
    ] {
        let text = truncate_width(text, inner_width);
        let padding = inner_width.saturating_sub(text_width(text.as_str()));
        lines.push(Line::from(vec![
            Span::styled("▎", bar),
            Span::styled("  ", slab),
            Span::styled(text, style),
            Span::styled(" ".repeat(padding), slab),
        ]));
    }
}

struct LineBuilder {
    prefix: Option<Prefix>,
    continuation_prefix: Option<String>,
    prefix_width: usize,
    max_width: usize,
    width: usize,
    spans: Vec<Span<'static>>,
    continuation: bool,
    pending_whitespace: Option<(String, Style, usize)>,
}

impl LineBuilder {
    fn new(prefix: Option<Prefix>, continuation_prefix: Option<String>, max_width: usize) -> Self {
        let prefix_width = prefix
            .as_ref()
            .map(|prefix| text_width(prefix.text.as_str()))
            .unwrap_or(0);
        let mut builder = Self {
            prefix,
            continuation_prefix,
            prefix_width,
            max_width,
            width: 0,
            spans: Vec::new(),
            continuation: false,
            pending_whitespace: None,
        };
        builder.reset();
        builder
    }

    fn push_text(&mut self, text: &str, style: Style, lines: &mut Vec<Line<'static>>) {
        let mut token = String::new();
        let mut token_is_whitespace = None;

        for grapheme in text.graphemes(true) {
            if grapheme == "\n" {
                self.push_token(&mut token, token_is_whitespace, style, lines);
                token_is_whitespace = None;
                self.finish(lines);
                continue;
            }

            let is_whitespace = grapheme.chars().all(char::is_whitespace);
            if token_is_whitespace != Some(is_whitespace) {
                self.push_token(&mut token, token_is_whitespace, style, lines);
                token_is_whitespace = Some(is_whitespace);
            }
            token.push_str(grapheme);
        }
        self.push_token(&mut token, token_is_whitespace, style, lines);
    }

    fn push_token(
        &mut self,
        token: &mut String,
        is_whitespace: Option<bool>,
        style: Style,
        lines: &mut Vec<Line<'static>>,
    ) {
        if token.is_empty() {
            return;
        }

        let token_width = text_width(token);
        if is_whitespace == Some(true) {
            if self.width > self.prefix_width {
                self.pending_whitespace = Some((std::mem::take(token), style, token_width));
            } else {
                token.clear();
            }
            return;
        }

        let pending_width = self
            .pending_whitespace
            .as_ref()
            .map(|(_, _, width)| *width)
            .unwrap_or(0);
        if self.width > self.prefix_width
            && self.width + pending_width + token_width > self.max_width
        {
            self.finish(lines);
        } else if let Some((mut whitespace, whitespace_style, whitespace_width)) =
            self.pending_whitespace.take()
        {
            self.flush(&mut whitespace, whitespace_style);
            self.width += whitespace_width;
        }

        let mut chunk = String::new();
        for grapheme in token.graphemes(true) {
            let grapheme_width = text_width(grapheme);
            if self.width > self.prefix_width && self.width + grapheme_width > self.max_width {
                self.flush(&mut chunk, style);
                self.finish(lines);
            }
            chunk.push_str(grapheme);
            self.width += grapheme_width;
        }
        self.flush(&mut chunk, style);
        token.clear();
    }

    fn flush(&mut self, chunk: &mut String, style: Style) {
        if !chunk.is_empty() {
            if let Some(previous) = self.spans.last_mut()
                && previous.style == style
            {
                previous.content.to_mut().push_str(chunk);
                chunk.clear();
            } else {
                self.spans.push(Span::styled(std::mem::take(chunk), style));
            }
        }
    }

    fn finish(&mut self, lines: &mut Vec<Line<'static>>) {
        self.pending_whitespace = None;
        lines.push(Line::from(std::mem::take(&mut self.spans)));
        self.continuation = true;
        self.reset();
    }

    fn reset(&mut self) {
        let prefix_text = if self.continuation {
            self.continuation_prefix
                .clone()
                .or_else(|| self.prefix.as_ref().map(|prefix| prefix.text.clone()))
        } else {
            self.prefix.as_ref().map(|prefix| prefix.text.clone())
        };
        let prefix_style = self
            .prefix
            .as_ref()
            .map(|prefix| prefix.style)
            .unwrap_or_default();
        self.prefix_width = prefix_text.as_deref().map(text_width).unwrap_or(0);
        self.width = self.prefix_width;
        self.pending_whitespace = None;
        self.spans = prefix_text
            .map(|prefix| vec![Span::styled(prefix, prefix_style)])
            .unwrap_or_default();
    }
}

fn render_code_block(
    language: Option<&str>,
    code: &str,
    max_width: usize,
    theme: Theme,
    lines: &mut Vec<Line<'static>>,
) {
    let bar = Style::default().fg(theme.accent_soft).bg(theme.surface);
    let slab = Style::default().bg(theme.surface);
    // One column for the bar, two of padding after it.
    let inner_width = max_width.saturating_sub(3);

    let label = truncate_width(language.unwrap_or("code"), inner_width);
    let label_pad = inner_width.saturating_sub(text_width(label.as_str()));
    lines.push(Line::from(vec![
        Span::styled("▎", bar),
        Span::styled("  ", slab),
        Span::styled(
            label,
            Style::default().fg(theme.text_muted).bg(theme.surface),
        ),
        Span::styled(" ".repeat(label_pad), slab),
    ]));

    for highlighted_line in syntax::highlight(language, code, theme) {
        let mut width = 0;
        let mut spans = vec![Span::styled("▎", bar), Span::styled("  ", slab)];
        for span in highlighted_line {
            let content = truncate_width(span.content.as_ref(), inner_width.saturating_sub(width));
            width += text_width(content.as_str());
            if !content.is_empty() {
                spans.push(Span::styled(content, span.style.bg(theme.surface)));
            }
        }
        spans.push(Span::styled(
            " ".repeat(inner_width.saturating_sub(width)),
            slab,
        ));
        lines.push(Line::from(spans));
    }

    // One blank slab row so the code never sits flush against the next block.
    lines.push(Line::from(vec![
        Span::styled("▎", bar),
        Span::styled(" ".repeat(max_width.saturating_sub(1)), slab),
    ]));
}

fn render_table(
    headers: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    max_width: usize,
    theme: Theme,
    lines: &mut Vec<Line<'static>>,
) {
    let mut table_rows = Vec::with_capacity(rows.len() + 1);
    let header = headers
        .iter()
        .map(|cell| table_cell_text(cell))
        .collect::<Vec<_>>();
    if !header.is_empty() {
        table_rows.push(header);
    }
    table_rows.extend(
        rows.iter()
            .map(|row| row.iter().map(|cell| table_cell_text(cell)).collect()),
    );

    let column_count = table_rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return;
    }
    let widths = table_column_widths(&table_rows, max_width);
    let ruled_width = widths
        .iter()
        .sum::<usize>()
        .saturating_add(
            widths
                .len()
                .saturating_sub(1)
                .saturating_mul(TABLE_GAP.len()),
        )
        .min(max_width);

    let first_body_row = usize::from(!headers.is_empty());
    let body = table_rows
        .iter()
        .skip(first_body_row)
        .map(|row| table_row(row, &widths, Style::default().fg(theme.text)))
        .collect::<Vec<_>>();
    let rule = |color| {
        Line::from(Span::styled(
            "─".repeat(ruled_width),
            Style::default().fg(color),
        ))
    };

    let mut table = Vec::new();
    if !headers.is_empty() {
        table.extend(table_row(
            &table_rows[0],
            &widths,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ));
        table.push(rule(theme.border));
    }
    // Every record is separated by a hairline. Whitespace alone is not enough
    // to track a row across a wide column gap, and it fails outright once a
    // cell wraps onto a second line.
    for (index, record) in body.into_iter().enumerate() {
        if index > 0 {
            table.push(rule(theme.reader_border));
        }
        table.extend(record);
    }

    // A table is sized by its content, so on a wide measure it would otherwise
    // sit against the left margin with the rest of the line empty beside it.
    // Centring it balances that space without stretching the columns apart.
    let indent = max_width.saturating_sub(ruled_width) / 2;
    if indent > 0 {
        let padding = " ".repeat(indent);
        for line in &mut table {
            line.spans.insert(0, Span::raw(padding.clone()));
        }
    }
    lines.extend(table);
}

fn table_cell_text(cell: &[Inline]) -> String {
    crate::markdown::inline_text(cell)
        .replace('\n', " ")
        .trim()
        .to_string()
}

/// Columns are separated by whitespace rather than by rules.
const TABLE_GAP: &str = "   ";

fn table_column_widths(rows: &[Vec<String>], max_width: usize) -> Vec<usize> {
    /// A column narrower than this wraps into confetti, so the row overflows
    /// the measure instead.
    const MIN_COLUMN: usize = 8;

    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = (0..column_count)
        .map(|column| {
            let mut cells = rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| text_width(cell.as_str()))
                .collect::<Vec<_>>();
            if cells.is_empty() {
                return 1;
            }
            let natural = cells.iter().copied().max().unwrap_or(1).max(1);
            cells.sort_unstable();
            let median = cells[cells.len() / 2];
            let header = rows
                .first()
                .and_then(|row| row.get(column))
                .map(|cell| text_width(cell.as_str()))
                .unwrap_or(0);
            // One long outlier should wrap rather than hold the whole column
            // open and leave a channel of whitespace beside every other row.
            natural.min(median.saturating_mul(2).max(header).max(MIN_COLUMN))
        })
        .collect::<Vec<_>>();
    let budget = max_width
        .saturating_sub(
            column_count
                .saturating_sub(1)
                .saturating_mul(TABLE_GAP.len()),
        )
        .max(column_count);

    // Take from the widest column first: cells wrap, so a narrow column costs
    // height rather than content.
    while widths.iter().sum::<usize>() > budget {
        let Some((index, width)) = widths.iter().enumerate().max_by_key(|(_, width)| **width)
        else {
            break;
        };
        if *width <= MIN_COLUMN {
            break;
        }
        widths[index] -= 1;
    }
    widths
}

/// Greedily wraps one cell to `width`, breaking inside a word only when the
/// word cannot fit on a line of its own.
fn wrap_cell(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = text_width(word);
        if current_width > 0 && current_width + 1 + word_width > width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let mut rest = word;
            while text_width(rest) > width {
                let head = truncate_width(rest, width);
                if head.is_empty() {
                    break;
                }
                rest = &rest[head.len()..];
                lines.push(head);
            }
            current.push_str(rest);
            current_width = text_width(rest);
            continue;
        }
        if current_width > 0 {
            current.push(' ');
            current_width += 1;
        }
        current.push_str(word);
        current_width += word_width;
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// One record, as however many lines its widest cell needs.
fn table_row(cells: &[String], widths: &[usize], cell_style: Style) -> Vec<Line<'static>> {
    let wrapped = widths
        .iter()
        .enumerate()
        .map(|(column, width)| {
            wrap_cell(cells.get(column).map(String::as_str).unwrap_or(""), *width)
        })
        .collect::<Vec<_>>();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);

    (0..height)
        .map(|row| {
            let mut spans = Vec::new();
            for (column, width) in widths.iter().enumerate() {
                if column > 0 {
                    spans.push(Span::styled(TABLE_GAP, cell_style));
                }
                let text = wrapped[column].get(row).cloned().unwrap_or_default();
                let padding = " ".repeat(width.saturating_sub(text_width(text.as_str())));
                spans.push(Span::styled(format!("{text}{padding}"), cell_style));
            }
            Line::from(spans)
        })
        .collect()
}

fn truncate_width(text: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut width = 0;
    for grapheme in text.graphemes(true) {
        let grapheme_width = text_width(grapheme);
        if width + grapheme_width > max_width {
            break;
        }
        result.push_str(grapheme);
        width += grapheme_width;
    }
    result
}

fn text_width(text: &str) -> usize {
    text.graphemes(true).map(UnicodeWidthStr::width).sum()
}

fn inline_style(style: InlineStyle, link: bool, base: Style, theme: Theme) -> Style {
    let mut result = base;
    if style.emphasis {
        result = result.add_modifier(Modifier::ITALIC);
    }
    if style.strong {
        result = result.add_modifier(Modifier::BOLD);
    }
    if style.strike {
        result = result.add_modifier(Modifier::CROSSED_OUT);
    }
    if style.code {
        result = result.fg(theme.code).bg(theme.surface_active);
    }
    if let Some(checked) = style.task {
        result = if checked {
            result.fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            result.fg(theme.text_muted)
        };
    }
    if link {
        result = result.fg(theme.link).add_modifier(Modifier::UNDERLINED);
    }
    result
}

/// Sub-headings below the ruled levels are marked by a tick in the left gutter
/// rather than by a prefix glyph inside the text, so the measure stays flush.
fn heading_tick(level: u8, theme: Theme) -> ratatui::style::Color {
    match level {
        3 => theme.accent_soft,
        _ => theme.border,
    }
}

fn heading_style(level: u8, theme: Theme) -> Style {
    match level {
        1 => Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        // `link` is reserved for things you can follow: sub-headings step down
        // through weight and the gutter tick instead.
        2 | 3 => Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ratatui::style::Modifier;
    use ratatui::text::Line;

    use super::{
        DocumentLayout, GUTTER_WIDTH, MAX_MEASURE, RIGHT_PAD, build, measure_for, text_width,
    };
    use crate::images::{Asset, ImageStore};
    use crate::markdown::Document;
    use crate::theme::Theme;

    /// The reader width that yields `measure` columns of text once the gutter
    /// and the right pad are taken out. Only used below the comfortable width,
    /// where the measure takes everything available.
    fn reader_width(measure: u16) -> u16 {
        measure + (GUTTER_WIDTH + RIGHT_PAD) as u16
    }

    #[test]
    fn the_measure_fills_the_terminal_up_to_the_ceiling() {
        // Every spare column goes to the text, at any ordinary width.
        assert_eq!(measure_for(60), 54);
        assert_eq!(measure_for(120), 114);
        assert_eq!(measure_for(200), 194);
        assert_eq!(measure_for(206), MAX_MEASURE);
        // Only an ultrawide window has anything left over.
        assert_eq!(measure_for(400), MAX_MEASURE);
    }

    /// One laid out line with its gutter stripped, as the measure reads it.
    fn measure_of(line: &Line<'_>, content_margin: usize) -> String {
        crate::selection::content_text(line, content_margin)
            .trim_end()
            .to_string()
    }

    fn measure(layout: &DocumentLayout, index: usize) -> String {
        measure_of(&layout.lines[index], layout.content_margin)
    }

    fn measures(layout: &DocumentLayout) -> Vec<String> {
        layout
            .lines
            .iter()
            .map(|line| measure_of(line, layout.content_margin))
            .collect()
    }

    #[test]
    fn wraps_text_to_the_available_width() {
        let document = Document::parse("A paragraph with enough words to wrap.");
        let layout = build(&document, 12, Theme::default(), &ImageStore::default());

        assert!(layout.lines.len() > 3);
    }

    #[test]
    fn wraps_between_words_before_splitting_them() {
        let layout = build(
            &Document::parse("alpha beta gamma delta"),
            reader_width(11),
            Theme::default(),
            &ImageStore::default(),
        );

        assert_eq!(measure(&layout, 0), "alpha beta");
        assert_eq!(measure(&layout, 1), "gamma delta");
    }

    #[test]
    fn tracks_exact_heading_lines_after_wrapping() {
        let document = Document::parse(
            "# First\n\nA paragraph with enough words to wrap several times.\n\n## Second",
        );
        let layout = build(&document, 14, Theme::default(), &ImageStore::default());

        assert_eq!(layout.heading_lines.len(), 2);
        assert!(layout.heading_line(1).unwrap() > layout.heading_line(0).unwrap());
    }

    #[test]
    fn renders_heading_text_without_markdown_markers_or_prefix_glyphs() {
        let theme = Theme::default();
        let layout = build(
            &Document::parse("# Title"),
            reader_width(12),
            theme,
            &ImageStore::default(),
        );

        // The marker never appears as a glyph in the text.
        assert_eq!(measure(&layout, 0), "Title");
        assert_eq!(measure(&layout, 1), "─".repeat(12));
        assert!(
            !layout.lines[0].spans.iter().any(|span| span.content == "▌"),
            "a ruled heading should not also carry a gutter tick"
        );

        // A sub-heading has no rule, so the gutter tick marks it instead.
        let layout = build(
            &Document::parse("### Sub"),
            reader_width(12),
            theme,
            &ImageStore::default(),
        );
        let tick = layout.lines[0]
            .spans
            .iter()
            .find(|span| span.content == "▌")
            .expect("a gutter tick on the sub-heading");
        assert_eq!(tick.style.fg, Some(theme.accent_soft));
    }

    #[test]
    fn gives_inline_code_a_surface_and_code_color() {
        let theme = Theme::default();
        let layout = build(
            &Document::parse("Use `cargo check` here."),
            40,
            theme,
            &ImageStore::default(),
        );
        let code_span = layout.lines[0]
            .spans
            .iter()
            .find(|span| span.content == "cargo check")
            .expect("inline code span");

        assert_eq!(code_span.style.bg, Some(theme.surface_active));
        assert_eq!(code_span.style.fg, Some(theme.code));
    }

    #[test]
    fn indents_wrapped_list_continuations() {
        let document = Document::parse("- A list item with enough text to wrap.");
        let layout = build(
            &document,
            reader_width(16),
            Theme::default(),
            &ImageStore::default(),
        );

        assert!(measure(&layout, 0).starts_with("•  "));
        assert!(measure(&layout, 1).starts_with("   "));
        assert!(!measure(&layout, 1).starts_with("•"));
    }

    #[test]
    fn centres_a_table_narrower_than_the_measure() {
        // Two three-column cells and one gap: nine columns of table inside a
        // sixty column measure, so twenty-five of indent on each side.
        let document = Document::parse("| A | B |\n| --- | --- |\n| one | two |");
        let layout = build(
            &document,
            reader_width(60),
            Theme::default(),
            &ImageStore::default(),
        );

        for needle in ["A", "one", "─"] {
            let line = layout
                .lines
                .iter()
                .map(|line| measure_of(line, layout.content_margin))
                .find(|text| text.trim_start().starts_with(needle))
                .unwrap_or_else(|| panic!("a table line starting with {needle:?}"));
            assert_eq!(
                line.len() - line.trim_start().len(),
                25,
                "{needle:?} is not centred: {line:?}"
            );
        }
    }

    #[test]
    fn separates_table_columns_with_space_rather_than_rules() {
        let document = Document::parse("| Name | Value |\n| --- | --- |\n| MarkR | reader |");
        let layout = build(
            &document,
            reader_width(24),
            Theme::default(),
            &ImageStore::default(),
        );
        let lines = measures(&layout);

        assert!(
            lines
                .iter()
                .any(|line| line.trim_start().starts_with("Name"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.trim_start().starts_with("MarkR"))
        );
        assert!(
            lines
                .iter()
                .all(|line| !line.contains('│') && !line.contains('┼') && !line.contains('├'))
        );
    }

    #[test]
    fn keeps_unicode_graphemes_together_when_wrapping() {
        let layout = build(
            &Document::parse("👩‍💻 ready"),
            reader_width(2),
            Theme::default(),
            &ImageStore::default(),
        );

        assert_eq!(text_width("e\u{301}"), 1);
        assert_eq!(text_width("👩‍💻"), 2);
        assert_eq!(measure(&layout, 0), "👩‍💻");
    }

    #[test]
    fn renders_blockquotes_with_a_softened_bar() {
        let theme = Theme::default();
        let layout = build(
            &Document::parse("> quoted words"),
            reader_width(40),
            theme,
            &ImageStore::default(),
        );
        let bar = layout.lines[0]
            .spans
            .iter()
            .find(|span| span.content == "▎  ")
            .expect("a quote bar");

        // The bar steps down from `accent` so quotes stop shouting.
        assert_eq!(bar.style.fg, Some(theme.accent_soft));
        assert_eq!(measure(&layout, 0), "▎  quoted words");
    }

    #[test]
    fn fills_code_blocks_with_a_slab_and_a_warm_bar() {
        let theme = Theme::default();
        let layout = build(
            &Document::parse("```rust\nlet value = 1;\n```"),
            reader_width(40),
            theme,
            &ImageStore::default(),
        );
        let lines = measures(&layout);

        assert!(lines.iter().any(|line| line.starts_with("▎  rust")));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("▎  let value = 1;"))
        );
        // No line art anywhere: the slab is a fill, not a frame.
        assert!(
            lines
                .iter()
                .all(|line| !line.contains('┌') && !line.contains('└') && !line.contains('│'))
        );

        let code_line = layout
            .lines
            .iter()
            .find(|line| line.to_string().contains("let value = 1;"))
            .expect("the code line");
        let bar = code_line
            .spans
            .iter()
            .position(|span| span.content == "▎")
            .expect("the slab bar");
        assert_eq!(code_line.spans[bar].style.fg, Some(theme.accent_soft));
        assert!(
            code_line.spans[bar..]
                .iter()
                .all(|span| span.style.bg == Some(theme.surface))
        );
        let slab_width: usize = code_line.spans[bar..]
            .iter()
            .map(|span| text_width(span.content.as_ref()))
            .sum();
        assert_eq!(slab_width, 40);
    }

    #[test]
    fn closes_h1_and_h2_with_a_hairline_rule() {
        let theme = Theme::default();
        let layout = build(
            &Document::parse("# One\n\n## Two\n\n### Three"),
            reader_width(12),
            theme,
            &ImageStore::default(),
        );

        // H1 and H2 each close with the same quiet rule; H1 is set apart by its
        // colour, not by a louder one. H3 gets no rule at all.
        let rules = layout
            .lines
            .iter()
            .filter(|line| measure_of(line, layout.content_margin).starts_with('─'))
            .collect::<Vec<_>>();
        assert_eq!(rules.len(), 2);
        for rule in rules {
            assert_eq!(measure_of(rule, layout.content_margin).chars().count(), 12);
            assert_eq!(
                rule.spans.last().expect("rule span").style.fg,
                Some(theme.reader_border)
            );
        }
    }

    #[test]
    fn wraps_table_cells_rather_than_cutting_them_short() {
        let document = Document::parse(
            "| Key | Action |\n| --- | --- |\n| Tab | Switch focus between the document and the sidebar |",
        );
        let layout = build(
            &document,
            reader_width(34),
            Theme::default(),
            &ImageStore::default(),
        );
        let rendered = measures(&layout).join(" ");

        // Every word survives; the cell gains a line instead of losing its end.
        for word in ["Switch", "focus", "between", "document", "sidebar"] {
            assert!(rendered.contains(word), "`{word}` was cut from the table");
        }
    }

    #[test]
    fn rules_the_table_header_and_every_record() {
        let theme = Theme::default();
        let document = Document::parse(
            "| Name | Value |\n| --- | --- |\n| MarkR | reader |\n| Ratatui | widgets |",
        );
        let layout = build(&document, reader_width(40), theme, &ImageStore::default());

        let rules = layout
            .lines
            .iter()
            .filter(|line| {
                let text = measure_of(line, layout.content_margin);
                let text = text.trim();
                !text.is_empty() && text.chars().all(|character| character == '─')
            })
            .collect::<Vec<_>>();

        // One under the header, then one between each pair of records: reading
        // across a wide column gap needs more than whitespace to follow.
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules[0].spans.last().expect("header rule").style.fg,
            Some(theme.border)
        );
        assert_eq!(
            rules[1].spans.last().expect("record rule").style.fg,
            Some(theme.reader_border)
        );

        let header = layout
            .lines
            .iter()
            .find(|line| {
                measure_of(line, layout.content_margin)
                    .trim_start()
                    .starts_with("Name")
            })
            .expect("the header row");
        assert!(
            header
                .spans
                .iter()
                .any(|span| span.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn centres_the_measure_only_once_it_stops_growing() {
        // At an ordinary width there is nothing left over to centre: the text
        // starts right after the gutter.
        let layout = build(
            &Document::parse("A short paragraph."),
            120,
            Theme::default(),
            &ImageStore::default(),
        );
        assert_eq!(layout.content_margin, GUTTER_WIDTH);

        // Past the ceiling the surplus becomes margin on both sides.
        let layout = build(
            &Document::parse("A short paragraph."),
            400,
            Theme::default(),
            &ImageStore::default(),
        );
        let margin = (400 - GUTTER_WIDTH - RIGHT_PAD - MAX_MEASURE) / 2;
        assert_eq!(layout.content_margin, margin + GUTTER_WIDTH);
        assert_eq!(layout.lines[0].spans[0].content, " ".repeat(margin));
    }

    #[test]
    fn reserves_rows_for_loaded_images() {
        let mut images = ImageStore::default();
        let document = Document::parse("![MarkR logo](markr-logo.png)");
        images.load(Some(Path::new("assets")), &document, 80);

        let layout = build(&document, 100, Theme::default(), &images);

        assert_eq!(layout.image_regions.len(), 1);
        let region = &layout.image_regions[0];
        let Some(Asset::Ready { rows, .. }) = images.asset(&region.src) else {
            panic!("expected a ready image asset");
        };
        assert!(*rows > 0);
        assert!(
            layout.lines[region.line..region.line + usize::from(*rows)]
                .iter()
                .all(|line| line.to_string().trim().is_empty())
        );
    }

    #[test]
    fn renders_placeholder_for_missing_images() {
        let document = Document::parse("![Ghost](missing.png)");
        let layout = build(
            &document,
            reader_width(40),
            Theme::default(),
            &ImageStore::default(),
        );
        let lines = measures(&layout);

        assert!(layout.image_regions.is_empty());
        assert!(lines.iter().any(|line| line.starts_with("▎  Ghost")));
        assert!(lines.iter().any(|line| line.contains("missing.png")));
        assert!(lines.iter().all(|line| !line.contains('┌')));
    }
}
