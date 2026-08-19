use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
                lines.push(Line::default());
                heading_lines.push(lines.len());
                let marker = format!("{} ", "#".repeat(*level as usize));
                push_wrapped_inlines(
                    content,
                    Some(marker),
                    heading_style(*level, theme),
                    max_width,
                    &mut lines,
                );
            }
            Block::Paragraph {
                content,
                quote_depth,
            } => {
                let prefix = if *quote_depth > 0 {
                    Some("│ ".to_string())
                } else {
                    None
                };
                push_wrapped_inlines(
                    content,
                    prefix,
                    Style::default().fg(theme.text),
                    max_width,
                    &mut lines,
                );
            }
            Block::List { ordered, items } => {
                for (index, item) in items.iter().enumerate() {
                    let marker = ordered
                        .map(|start| format!("{}.", start + index as u64))
                        .unwrap_or_else(|| "•".to_string());
                    push_wrapped_inlines(
                        item,
                        Some(format!("{marker} ")),
                        Style::default().fg(theme.text),
                        max_width,
                        &mut lines,
                    );
                }
            }
            Block::FencedCode { language, code } => {
                let label = language.as_deref().unwrap_or("code");
                lines.push(Line::from(Span::styled(
                    format!("  ┌─ {label}"),
                    Style::default().fg(theme.code),
                )));
                for highlighted_line in syntax::highlight(language.as_deref(), code, theme) {
                    let mut code_spans =
                        vec![Span::styled("  │ ", Style::default().fg(theme.code))];
                    code_spans.extend(highlighted_line);
                    lines.push(Line::from(code_spans));
                }
                lines.push(Line::from(Span::styled(
                    "  └─",
                    Style::default().fg(theme.code),
                )));
            }
            Block::Table { headers, rows } => {
                let header = headers
                    .iter()
                    .map(|cell| crate::markdown::inline_text(cell))
                    .collect::<Vec<_>>()
                    .join("  │  ");
                if !header.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("│ {header} │"),
                        theme.title(),
                    )));
                    lines.push(Line::from(Span::styled(
                        "├──────────────────────────────┤",
                        theme.muted(),
                    )));
                }
                for row in rows {
                    let content = row
                        .iter()
                        .map(|cell| crate::markdown::inline_text(cell))
                        .collect::<Vec<_>>()
                        .join("  │  ");
                    lines.push(Line::from(Span::styled(
                        format!("│ {content} │"),
                        Style::default().fg(theme.text),
                    )));
                }
            }
            Block::ThematicBreak => lines.push(Line::from(Span::styled(
                "  ─────────────────────────────",
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
    base_style: Style,
    max_width: usize,
    lines: &mut Vec<Line<'static>>,
) {
    let mut builder = LineBuilder::new(prefix, max_width);
    for inline in content {
        builder.push_text(
            &inline.text,
            inline_style(inline.style, inline.link.is_some(), base_style),
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
    let mut builder = LineBuilder::new(prefix, max_width);
    builder.push_text(text, style, lines);
    builder.finish(lines);
}

struct LineBuilder {
    prefix: Option<String>,
    prefix_width: usize,
    max_width: usize,
    width: usize,
    spans: Vec<Span<'static>>,
}

impl LineBuilder {
    fn new(prefix: Option<String>, max_width: usize) -> Self {
        let prefix_width = prefix.as_deref().map(UnicodeWidthStr::width).unwrap_or(0);
        let mut builder = Self {
            prefix,
            prefix_width,
            max_width,
            width: 0,
            spans: Vec::new(),
        };
        builder.reset();
        builder
    }

    fn push_text(&mut self, text: &str, style: Style, lines: &mut Vec<Line<'static>>) {
        let mut chunk = String::new();
        for character in text.chars() {
            if character == '\n' {
                self.flush(&mut chunk, style);
                self.finish(lines);
                continue;
            }

            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            let would_overflow = self.width + character_width > self.max_width;
            if would_overflow && self.width > self.prefix_width {
                self.flush(&mut chunk, style);
                self.finish(lines);
                if character.is_whitespace() {
                    continue;
                }
            }

            chunk.push(character);
            self.width += character_width;
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
        self.reset();
    }

    fn reset(&mut self) {
        self.width = self.prefix_width;
        self.spans = self
            .prefix
            .as_ref()
            .map(|prefix| vec![Span::raw(prefix.clone())])
            .unwrap_or_default();
    }
}

fn inline_style(style: InlineStyle, link: bool, base: Style) -> Style {
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
        result = result.fg(ratatui::style::Color::Rgb(219, 180, 108));
    }
    if link {
        result = result
            .fg(ratatui::style::Color::Rgb(107, 174, 255))
            .add_modifier(Modifier::UNDERLINED);
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
    use super::build;
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
}
