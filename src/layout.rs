use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::markdown::{Block, Document, Inline, InlineStyle};
use crate::syntax;
use crate::theme::Theme;

#[derive(Debug)]
pub struct DocumentLayout {
    pub lines: Vec<Line<'static>>,
    pub heading_lines: Vec<usize>,
}

impl DocumentLayout {
    pub fn heading_line(&self, heading_index: usize) -> Option<usize> {
        self.heading_lines.get(heading_index).copied()
    }
}

pub fn build(document: &Document, width: u16, theme: Theme) -> DocumentLayout {
    let max_width = usize::from(width.max(1));
    let mut lines = Vec::new();
    let mut heading_lines = Vec::new();

    for block in &document.blocks {
        match block {
            Block::Heading { level, content } => {
                heading_lines.push(lines.len());
                push_wrapped_inlines(
                    content,
                    None,
                    None,
                    heading_style(*level, theme),
                    max_width,
                    theme,
                    &mut lines,
                );
                if *level == 1 {
                    lines.push(Line::from(Span::styled(
                        "─".repeat(max_width),
                        theme.border,
                    )));
                }
            }
            Block::Paragraph {
                content,
                quote_depth,
            } => {
                let prefix = if *quote_depth > 0 {
                    Some(format!(
                        "{}│ ",
                        "  ".repeat(quote_depth.saturating_sub(1) as usize)
                    ))
                } else {
                    None
                };
                let base_style = if *quote_depth > 0 {
                    Style::default()
                        .fg(theme.text_muted)
                        .add_modifier(Modifier::ITALIC)
                } else {
                    Style::default().fg(theme.text)
                };
                push_wrapped_inlines(
                    content,
                    prefix.clone(),
                    prefix,
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
                    let prefix = format!("{marker} ");
                    let continuation = " ".repeat(text_width(prefix.as_str()));
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
                let label = language.as_deref().unwrap_or("code");
                lines.push(Line::from(Span::styled(
                    format!("  ┌─ {label}"),
                    Style::default().fg(theme.code).bg(theme.surface),
                )));
                for highlighted_line in syntax::highlight(language.as_deref(), code, theme) {
                    let mut code_spans = vec![Span::styled(
                        "  │ ",
                        Style::default().fg(theme.code).bg(theme.surface),
                    )];
                    code_spans.extend(highlighted_line);
                    lines.push(Line::from(code_spans));
                }
                lines.push(Line::from(Span::styled(
                    "  └─",
                    Style::default().fg(theme.code).bg(theme.surface),
                )));
            }
            Block::Table { headers, rows } => {
                render_table(headers, rows, max_width, theme, &mut lines);
            }
            Block::ThematicBreak => lines.push(Line::from(Span::styled(
                "─".repeat(max_width),
                theme.border,
            ))),
            Block::Html(html) => {
                push_wrapped_text(html, None, theme.muted(), max_width, &mut lines)
            }
        }
        lines.push(Line::default());
    }

    DocumentLayout {
        lines,
        heading_lines,
    }
}

fn push_wrapped_inlines(
    content: &[Inline],
    prefix: Option<String>,
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

fn push_wrapped_text(
    text: &str,
    prefix: Option<String>,
    style: Style,
    max_width: usize,
    lines: &mut Vec<Line<'static>>,
) {
    let mut builder = LineBuilder::new(prefix, None, max_width);
    builder.push_text(text, style, lines);
    builder.finish(lines);
}

struct LineBuilder {
    prefix: Option<String>,
    continuation_prefix: Option<String>,
    prefix_width: usize,
    max_width: usize,
    width: usize,
    spans: Vec<Span<'static>>,
    continuation: bool,
}

impl LineBuilder {
    fn new(prefix: Option<String>, continuation_prefix: Option<String>, max_width: usize) -> Self {
        let prefix_width = prefix.as_deref().map(text_width).unwrap_or(0);
        let mut builder = Self {
            prefix,
            continuation_prefix,
            prefix_width,
            max_width,
            width: 0,
            spans: Vec::new(),
            continuation: false,
        };
        builder.reset();
        builder
    }

    fn push_text(&mut self, text: &str, style: Style, lines: &mut Vec<Line<'static>>) {
        let mut chunk = String::new();
        for grapheme in text.graphemes(true) {
            if grapheme == "\n" {
                self.flush(&mut chunk, style);
                self.finish(lines);
                continue;
            }

            let grapheme_width = text_width(grapheme);
            let would_overflow = self.width + grapheme_width > self.max_width;
            if would_overflow && self.width > self.prefix_width {
                self.flush(&mut chunk, style);
                self.finish(lines);
                if grapheme.chars().all(char::is_whitespace) {
                    continue;
                }
            }

            chunk.push_str(grapheme);
            self.width += grapheme_width;
        }
        self.flush(&mut chunk, style);
    }

    fn flush(&mut self, chunk: &mut String, style: Style) {
        if !chunk.is_empty() {
            self.spans.push(Span::styled(std::mem::take(chunk), style));
        }
    }

    fn finish(&mut self, lines: &mut Vec<Line<'static>>) {
        lines.push(Line::from(std::mem::take(&mut self.spans)));
        self.continuation = true;
        self.reset();
    }

    fn reset(&mut self) {
        let prefix = if self.continuation {
            self.continuation_prefix.as_ref().or(self.prefix.as_ref())
        } else {
            self.prefix.as_ref()
        };
        self.prefix_width = prefix
            .map(|prefix| text_width(prefix.as_str()))
            .unwrap_or(0);
        self.width = self.prefix_width;
        self.spans = prefix
            .map(|prefix| vec![Span::raw(prefix.clone())])
            .unwrap_or_default();
    }
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

    if !headers.is_empty() {
        lines.push(table_row(&table_rows[0], &widths, theme.title()));
        lines.push(table_separator(&widths, theme.muted()));
    }
    let first_body_row = usize::from(!headers.is_empty());
    for row in table_rows.iter().skip(first_body_row) {
        lines.push(table_row(row, &widths, Style::default().fg(theme.text)));
    }
}

fn table_cell_text(cell: &[Inline]) -> String {
    crate::markdown::inline_text(cell)
        .replace('\n', " ")
        .trim()
        .to_string()
}

fn table_column_widths(rows: &[Vec<String>], max_width: usize) -> Vec<usize> {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = (0..column_count)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| text_width(cell.as_str()))
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect::<Vec<_>>();
    let budget = max_width
        .saturating_sub(column_count.saturating_mul(3).saturating_add(1))
        .max(column_count);

    while widths.iter().sum::<usize>() > budget {
        let Some((index, width)) = widths.iter().enumerate().max_by_key(|(_, width)| **width)
        else {
            break;
        };
        if *width <= 1 {
            break;
        }
        widths[index] -= 1;
    }
    widths
}

fn table_row(cells: &[String], widths: &[usize], style: Style) -> Line<'static> {
    let mut content = String::from("│");
    for (column, width) in widths.iter().enumerate() {
        let cell = cells.get(column).map(String::as_str).unwrap_or("");
        let cell = truncate_width(cell, *width);
        content.push(' ');
        content.push_str(&cell);
        content.push_str(&" ".repeat(width.saturating_sub(text_width(cell.as_str())) + 1));
        content.push('│');
    }
    Line::from(Span::styled(content, style))
}

fn table_separator(widths: &[usize], style: Style) -> Line<'static> {
    let mut content = String::from("├");
    for (index, width) in widths.iter().enumerate() {
        content.push_str(&"─".repeat(width + 2));
        if index + 1 < widths.len() {
            content.push('┼');
        }
    }
    content.push('┤');
    Line::from(Span::styled(content, style))
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

fn heading_style(level: u8, theme: Theme) -> Style {
    match level {
        1 => Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        2 => Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use super::{build, text_width};
    use crate::markdown::Document;
    use crate::theme::Theme;

    #[test]
    fn wraps_text_to_the_available_width() {
        let document = Document::parse("A paragraph with enough words to wrap.");
        let layout = build(&document, 12, Theme::default());

        assert!(layout.lines.len() > 3);
    }

    #[test]
    fn tracks_exact_heading_lines_after_wrapping() {
        let document = Document::parse(
            "# First\n\nA paragraph with enough words to wrap several times.\n\n## Second",
        );
        let layout = build(&document, 14, Theme::default());

        assert_eq!(layout.heading_lines.len(), 2);
        assert!(layout.heading_line(1).unwrap() > layout.heading_line(0).unwrap());
    }

    #[test]
    fn renders_heading_text_without_markdown_markers() {
        let layout = build(&Document::parse("# Title"), 12, Theme::default());

        assert_eq!(layout.lines[0].to_string(), "Title");
        assert_eq!(layout.lines[1].to_string(), "────────────");
    }

    #[test]
    fn gives_inline_code_a_surface_and_code_color() {
        let theme = Theme::default();
        let layout = build(&Document::parse("Use `cargo check` here."), 40, theme);
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
        let layout = build(&document, 16, Theme::default());

        assert!(layout.lines[0].to_string().starts_with("• "));
        assert!(layout.lines[1].to_string().starts_with("  "));
        assert!(!layout.lines[1].to_string().starts_with("• "));
    }

    #[test]
    fn renders_table_columns_with_intersections() {
        let document = Document::parse("| Name | Value |\n| --- | --- |\n| MarkR | reader |");
        let layout = build(&document, 24, Theme::default());

        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().starts_with("├") && line.to_string().contains('┼'))
        );
    }

    #[test]
    fn keeps_unicode_graphemes_together_when_wrapping() {
        let layout = build(&Document::parse("👩‍💻 ready"), 2, Theme::default());

        assert_eq!(text_width("e\u{301}"), 1);
        assert_eq!(text_width("👩‍💻"), 2);
        assert_eq!(layout.lines[0].to_string(), "👩‍💻");
    }
}
