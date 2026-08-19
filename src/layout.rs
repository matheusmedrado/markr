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

pub const MAX_CONTENT_WIDTH: usize = 88;

pub fn build(document: &Document, width: u16, theme: Theme, images: &ImageStore) -> DocumentLayout {
    let total_width = usize::from(width.max(1));
    let max_width = total_width.min(MAX_CONTENT_WIDTH);
    let mut lines = Vec::new();
    let mut heading_lines = Vec::new();
    let mut image_regions = Vec::new();

    for block in &document.blocks {
        match block {
            Block::Heading { level, content } => {
                if !lines.is_empty() {
                    lines.push(Line::default());
                }
                heading_lines.push(lines.len());
                let prefix = heading_prefix(*level, theme);
                let continuation = prefix
                    .as_ref()
                    .map(|prefix| " ".repeat(text_width(prefix.text.as_str())));
                push_wrapped_inlines(
                    content,
                    prefix,
                    continuation,
                    heading_style(*level, theme),
                    max_width,
                    theme,
                    &mut lines,
                );
                if *level == 1 {
                    lines.push(Line::from(Span::styled(
                        "─".repeat(max_width),
                        Style::default().fg(theme.border),
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
                            text: "▎ ".repeat(*quote_depth as usize),
                            style: Style::default().fg(theme.accent),
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
                        text: format!("{marker} "),
                        style: Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
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
                Style::default().fg(theme.border),
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

    let margin = total_width.saturating_sub(max_width) / 2;
    if margin > 0 {
        let indent = " ".repeat(margin);
        for line in &mut lines {
            line.spans.insert(0, Span::raw(indent.clone()));
        }
    }

    DocumentLayout {
        lines,
        heading_lines,
        image_regions,
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
    let frame_style = Style::default().fg(theme.border);
    let title = if alt.is_empty() { src } else { alt };
    let title = truncate_width(title, max_width.saturating_sub(4));

    let top_fill = max_width.saturating_sub(4 + text_width(title.as_str()));
    lines.push(Line::from(vec![
        Span::styled("┌─ ", frame_style),
        Span::styled(title, Style::default().fg(theme.text_muted)),
        Span::styled(format!(" {}", "─".repeat(top_fill)), frame_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("│ ", frame_style),
        Span::styled(
            truncate_width(src, max_width.saturating_sub(2)),
            theme.muted(),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!("└─{}", "─".repeat(max_width.saturating_sub(2))),
        frame_style,
    )));
}

struct LineBuilder {
    prefix: Option<Prefix>,
    continuation_prefix: Option<String>,
    prefix_width: usize,
    max_width: usize,
    width: usize,
    spans: Vec<Span<'static>>,
    continuation: bool,
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
    let label = language.unwrap_or("code");
    let frame_style = Style::default().fg(theme.border).bg(theme.surface);
    let label_style = Style::default()
        .fg(theme.code)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);

    let header_width = 4 + text_width(label);
    lines.push(Line::from(vec![
        Span::styled("┌─ ", frame_style),
        Span::styled(label.to_string(), label_style),
        Span::styled(
            format!(" {}", "─".repeat(max_width.saturating_sub(header_width))),
            frame_style,
        ),
    ]));

    let fill_style = Style::default().bg(theme.surface);
    for highlighted_line in syntax::highlight(language, code, theme) {
        let mut line_width = 2;
        let mut code_spans = vec![Span::styled("│ ", frame_style)];
        for span in highlighted_line {
            line_width += text_width(span.content.as_ref());
            code_spans.push(span);
        }
        code_spans.push(Span::styled(
            " ".repeat(max_width.saturating_sub(line_width)),
            fill_style,
        ));
        lines.push(Line::from(code_spans));
    }

    lines.push(Line::from(Span::styled(
        format!("└─{}", "─".repeat(max_width.saturating_sub(2))),
        frame_style,
    )));
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

    let border_style = Style::default().fg(theme.border);
    lines.push(table_edge(&widths, '┌', '┬', '┐', border_style));
    if !headers.is_empty() {
        lines.push(table_row(
            &table_rows[0],
            &widths,
            theme.accent(),
            border_style,
        ));
        lines.push(table_edge(&widths, '├', '┼', '┤', border_style));
    }
    let first_body_row = usize::from(!headers.is_empty());
    for (index, row) in table_rows.iter().skip(first_body_row).enumerate() {
        let cell_style = if index % 2 == 1 {
            Style::default().fg(theme.text).bg(theme.surface)
        } else {
            Style::default().fg(theme.text)
        };
        lines.push(table_row(row, &widths, cell_style, border_style));
    }
    lines.push(table_edge(&widths, '└', '┴', '┘', border_style));
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

fn table_row(
    cells: &[String],
    widths: &[usize],
    cell_style: Style,
    border_style: Style,
) -> Line<'static> {
    let mut spans = vec![Span::styled("│", border_style)];
    for (column, width) in widths.iter().enumerate() {
        let cell = cells.get(column).map(String::as_str).unwrap_or("");
        let cell = truncate_width(cell, *width);
        let padding = " ".repeat(width.saturating_sub(text_width(cell.as_str())));
        spans.push(Span::styled(format!(" {cell}{padding} "), cell_style));
        spans.push(Span::styled("│", border_style));
    }
    Line::from(spans)
}

fn table_edge(
    widths: &[usize],
    left: char,
    joint: char,
    right: char,
    style: Style,
) -> Line<'static> {
    let mut content = String::new();
    content.push(left);
    for (index, width) in widths.iter().enumerate() {
        content.push_str(&"─".repeat(width + 2));
        if index + 1 < widths.len() {
            content.push(joint);
        }
    }
    content.push(right);
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

fn heading_prefix(level: u8, theme: Theme) -> Option<Prefix> {
    match level {
        2 => Some(Prefix {
            text: "❯ ".to_string(),
            style: Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        }),
        3..=6 => Some(Prefix {
            text: "▹ ".to_string(),
            style: Style::default().fg(theme.text_muted),
        }),
        _ => None,
    }
}

fn heading_style(level: u8, theme: Theme) -> Style {
    match level {
        1 => Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        2 => Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        3 => Style::default().fg(theme.link).add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{build, text_width};
    use crate::images::{Asset, ImageStore};
    use crate::markdown::Document;
    use crate::theme::Theme;

    #[test]
    fn wraps_text_to_the_available_width() {
        let document = Document::parse("A paragraph with enough words to wrap.");
        let layout = build(&document, 12, Theme::default(), &ImageStore::default());

        assert!(layout.lines.len() > 3);
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
    fn renders_heading_text_without_markdown_markers() {
        let layout = build(
            &Document::parse("# Title"),
            12,
            Theme::default(),
            &ImageStore::default(),
        );

        assert_eq!(layout.lines[0].to_string(), "Title");
        assert_eq!(layout.lines[1].to_string(), "────────────");
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
        let layout = build(&document, 16, Theme::default(), &ImageStore::default());

        assert!(layout.lines[0].to_string().starts_with("• "));
        assert!(layout.lines[1].to_string().starts_with("  "));
        assert!(!layout.lines[1].to_string().starts_with("• "));
    }

    #[test]
    fn renders_table_columns_with_intersections() {
        let document = Document::parse("| Name | Value |\n| --- | --- |\n| MarkR | reader |");
        let layout = build(&document, 24, Theme::default(), &ImageStore::default());

        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().starts_with("├") && line.to_string().contains('┼'))
        );
    }

    #[test]
    fn keeps_unicode_graphemes_together_when_wrapping() {
        let layout = build(
            &Document::parse("👩‍💻 ready"),
            2,
            Theme::default(),
            &ImageStore::default(),
        );

        assert_eq!(text_width("e\u{301}"), 1);
        assert_eq!(text_width("👩‍💻"), 2);
        assert_eq!(layout.lines[0].to_string(), "👩‍💻");
    }

    #[test]
    fn renders_blockquotes_with_an_accent_bar() {
        let theme = Theme::default();
        let layout = build(
            &Document::parse("> quoted words"),
            40,
            theme,
            &ImageStore::default(),
        );
        let bar_span = &layout.lines[0].spans[0];

        assert_eq!(bar_span.content, "▎ ");
        assert_eq!(bar_span.style.fg, Some(theme.accent));
        assert_eq!(layout.lines[0].to_string(), "▎ quoted words");
    }

    #[test]
    fn fills_code_blocks_with_the_surface_background() {
        let theme = Theme::default();
        let layout = build(
            &Document::parse("```rust\nlet value = 1;\n```"),
            40,
            theme,
            &ImageStore::default(),
        );

        let code_line = layout
            .lines
            .iter()
            .find(|line| line.to_string().starts_with("│ "))
            .expect("a gutter line");
        let total_width: usize = code_line
            .spans
            .iter()
            .map(|span| text_width(span.content.as_ref()))
            .sum();
        assert_eq!(total_width, 40);
        assert!(
            code_line
                .spans
                .iter()
                .all(|span| span.style.bg == Some(theme.surface))
        );
        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().starts_with("┌─ rust"))
        );
        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().starts_with("└─"))
        );
    }

    #[test]
    fn frames_tables_top_and_bottom() {
        let document = Document::parse("| Name | Value |\n| --- | --- |\n| MarkR | reader |");
        let layout = build(&document, 40, Theme::default(), &ImageStore::default());

        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().starts_with('┌'))
        );
        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().starts_with('└'))
        );
    }

    #[test]
    fn centers_the_document_on_wide_terminals() {
        let layout = build(
            &Document::parse("A short paragraph."),
            120,
            Theme::default(),
            &ImageStore::default(),
        );
        let margin = (120 - 88) / 2;

        assert_eq!(layout.lines[0].spans[0].content, " ".repeat(margin));
    }

    #[test]
    fn reserves_rows_for_loaded_images() {
        let mut images = ImageStore::default();
        let document = Document::parse("![MarkR logo](markr-logo.png)");
        images.load(Some(Path::new("assets")), &document);

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
        let layout = build(&document, 40, Theme::default(), &ImageStore::default());

        assert!(layout.image_regions.is_empty());
        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().starts_with("┌─ Ghost"))
        );
        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.to_string().contains("missing.png"))
        );
    }
}
