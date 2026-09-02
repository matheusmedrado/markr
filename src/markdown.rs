use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub outline: Vec<Heading>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub block_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph {
        content: Vec<Inline>,
        quote_depth: u8,
    },
    List {
        ordered: Option<u64>,
        items: Vec<Vec<Inline>>,
    },
    FencedCode {
        language: Option<String>,
        code: String,
    },
    Table {
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    ThematicBreak,
    Image {
        src: String,
        alt: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub style: InlineStyle,
    pub link: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InlineStyle {
    pub emphasis: bool,
    pub strong: bool,
    pub strike: bool,
    pub code: bool,
    pub task: Option<bool>,
}

impl Document {
    pub fn parse(source: &str) -> Self {
        let mut blocks = Vec::new();
        let mut current = None;
        let mut quote_depth = 0;
        let mut style = InlineStyle::default();
        let mut style_stack = Vec::new();
        let mut link = None;
        let mut link_stack = Vec::new();
        let mut table_context = TableContext::default();
        let mut images: Vec<ImageWork> = Vec::new();

        let mut options = Options::all();
        options.insert(Options::ENABLE_TASKLISTS);
        let parser = Parser::new_ext(source, options);

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading(level, _, _) => {
                        current = Some(WorkingBlock::Heading {
                            level: heading_level(level),
                            content: Vec::new(),
                        });
                    }
                    Tag::Paragraph if !matches!(current, Some(WorkingBlock::List { .. })) => {
                        current = Some(WorkingBlock::Paragraph {
                            content: Vec::new(),
                            quote_depth,
                        });
                    }
                    Tag::BlockQuote => quote_depth = quote_depth.saturating_add(1),
                    Tag::List(ordered) => {
                        if !matches!(current, Some(WorkingBlock::List { .. })) {
                            current = Some(WorkingBlock::List {
                                ordered,
                                items: Vec::new(),
                            });
                        }
                    }
                    Tag::Item => {
                        if let Some(WorkingBlock::List { items, .. }) = current.as_mut() {
                            items.push(Vec::new());
                        }
                    }
                    Tag::CodeBlock(kind) => {
                        let language = match kind {
                            CodeBlockKind::Fenced(info) if !info.is_empty() => {
                                Some(info.into_string())
                            }
                            _ => None,
                        };
                        current = Some(WorkingBlock::FencedCode {
                            language,
                            code: String::new(),
                        });
                    }
                    Tag::Emphasis => {
                        push_style(&mut style, &mut style_stack, |value| value.emphasis = true)
                    }
                    Tag::Strong => {
                        push_style(&mut style, &mut style_stack, |value| value.strong = true)
                    }
                    Tag::Strikethrough => {
                        push_style(&mut style, &mut style_stack, |value| value.strike = true)
                    }
                    Tag::Link(_, destination, _) => {
                        link_stack.push(link.take());
                        link = Some(destination.into_string());
                    }
                    Tag::Image(_, destination, _) => {
                        images.push(ImageWork {
                            src: destination.into_string(),
                            marker: current_content_len(&current),
                            alt: String::new(),
                        });
                    }
                    Tag::Table(_) => {
                        table_context = TableContext::default();
                        current = Some(WorkingBlock::Table {
                            headers: Vec::new(),
                            rows: Vec::new(),
                        });
                    }
                    Tag::TableHead => table_context.in_head = true,
                    Tag::TableRow => table_context.current_row.clear(),
                    Tag::TableCell => table_context.current_cell.clear(),
                    _ => {}
                },
                Event::End(tag) => match tag {
                    Tag::Heading(..)
                    | Tag::Paragraph
                    | Tag::List(_)
                    | Tag::CodeBlock(_)
                    | Tag::Table(_) => {
                        finish_current(&mut current, &mut blocks);
                    }
                    Tag::BlockQuote => quote_depth = quote_depth.saturating_sub(1),
                    Tag::Item => {}
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough => {
                        if let Some(previous) = style_stack.pop() {
                            style = previous;
                        }
                    }
                    Tag::Link(..) => link = link_stack.pop().flatten(),
                    Tag::Image(..) => {
                        if let Some(work) = images.pop() {
                            let replaces_current = matches!(
                                &current,
                                Some(WorkingBlock::Paragraph { content, .. })
                                    if content.len() == work.marker
                            );
                            if replaces_current {
                                current = None;
                            } else {
                                finish_current(&mut current, &mut blocks);
                            }
                            blocks.push(Block::Image {
                                src: work.src,
                                alt: work.alt,
                            });
                        }
                    }
                    Tag::TableHead => {
                        table_context.in_head = false;
                        if let Some(WorkingBlock::Table { headers, .. }) = current.as_mut() {
                            *headers = std::mem::take(&mut table_context.current_row);
                        }
                    }
                    Tag::TableRow => {
                        if let Some(WorkingBlock::Table { headers, rows }) = current.as_mut() {
                            let row = std::mem::take(&mut table_context.current_row);
                            if table_context.in_head {
                                *headers = row;
                            } else {
                                rows.push(row);
                            }
                        }
                    }
                    Tag::TableCell => {
                        table_context
                            .current_row
                            .push(std::mem::take(&mut table_context.current_cell));
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if let Some(work) = images.last_mut() {
                        work.alt.push_str(text.as_ref());
                    } else {
                        append_text(
                            &mut current,
                            text.as_ref(),
                            style,
                            link.clone(),
                            &mut table_context,
                        );
                    }
                }
                Event::Code(text) => {
                    if let Some(work) = images.last_mut() {
                        work.alt.push_str(text.as_ref());
                    } else {
                        append_text(
                            &mut current,
                            text.as_ref(),
                            InlineStyle {
                                code: true,
                                ..InlineStyle::default()
                            },
                            link.clone(),
                            &mut table_context,
                        );
                    }
                }
                Event::Html(html) => {
                    let img_tags = extract_img_tags(html.as_ref());
                    if !img_tags.is_empty() {
                        finish_current(&mut current, &mut blocks);
                        for (src, alt) in img_tags {
                            blocks.push(Block::Image { src, alt });
                        }
                    }
                }
                // A soft break is a wrapped source line, not a line break in the
                // prose: it reflows as a space so paragraphs fill the measure.
                // Only an explicit hard break survives to the renderer.
                Event::SoftBreak | Event::HardBreak => {
                    let separator = if matches!(event, Event::HardBreak) {
                        "\n"
                    } else {
                        " "
                    };
                    if let Some(work) = images.last_mut() {
                        work.alt.push(' ');
                    } else {
                        append_text(
                            &mut current,
                            separator,
                            style,
                            link.clone(),
                            &mut table_context,
                        );
                    }
                }
                Event::Rule => blocks.push(Block::ThematicBreak),
                Event::TaskListMarker(checked) => append_text(
                    &mut current,
                    if checked { "☑ " } else { "☐ " },
                    InlineStyle {
                        task: Some(checked),
                        ..style
                    },
                    link.clone(),
                    &mut table_context,
                ),
                Event::FootnoteReference(name) => append_text(
                    &mut current,
                    &format!("[{}]", name),
                    style,
                    link.clone(),
                    &mut table_context,
                ),
            }
        }

        finish_current(&mut current, &mut blocks);
        let outline = blocks
            .iter()
            .enumerate()
            .filter_map(|(block_index, block)| match block {
                Block::Heading { level, content } => Some(Heading {
                    level: *level,
                    title: inline_text(content),
                    block_index,
                }),
                _ => None,
            })
            .collect();

        Self { blocks, outline }
    }
}

#[derive(Debug)]
enum WorkingBlock {
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph {
        content: Vec<Inline>,
        quote_depth: u8,
    },
    List {
        ordered: Option<u64>,
        items: Vec<Vec<Inline>>,
    },
    FencedCode {
        language: Option<String>,
        code: String,
    },
    Table {
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
}

#[derive(Default)]
struct TableContext {
    in_head: bool,
    current_row: Vec<Vec<Inline>>,
    current_cell: Vec<Inline>,
}

struct ImageWork {
    src: String,
    marker: usize,
    alt: String,
}

fn current_content_len(current: &Option<WorkingBlock>) -> usize {
    match current {
        Some(WorkingBlock::Paragraph { content, .. })
        | Some(WorkingBlock::Heading { content, .. }) => content.len(),
        _ => usize::MAX,
    }
}

fn extract_img_tags(html: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find("<img") {
        let start = search_from + offset;
        let Some(tag_end) = html[start..].find('>').map(|end| start + end) else {
            break;
        };
        let tag = &html[start + 4..tag_end];
        let is_tag_boundary = tag
            .as_bytes()
            .first()
            .is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>'));
        if is_tag_boundary {
            let src = html_attribute(tag, "src").unwrap_or_default();
            let alt = html_attribute(tag, "alt").unwrap_or_default();
            if !src.is_empty() {
                results.push((src, alt));
            }
        }
        search_from = tag_end + 1;
    }
    results
}

fn html_attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find(name) {
        let start = search_from + offset;
        let end = start + name.len();
        let boundary_ok = start == 0
            || tag[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        search_from = end;
        if !boundary_ok {
            continue;
        }
        let rest = tag[end..].trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        return match value.as_bytes().first() {
            Some(b'"') | Some(b'\'') => {
                let quote = value.as_bytes()[0] as char;
                let inner = &value[1..];
                inner.find(quote).map(|stop| inner[..stop].to_string())
            }
            _ => Some(
                value
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            ),
        };
    }
    None
}

fn finish_current(current: &mut Option<WorkingBlock>, blocks: &mut Vec<Block>) {
    let Some(block) = current.take() else { return };
    match block {
        WorkingBlock::Heading { level, content } => blocks.push(Block::Heading { level, content }),
        WorkingBlock::Paragraph {
            content,
            quote_depth,
        } => blocks.push(Block::Paragraph {
            content,
            quote_depth,
        }),
        WorkingBlock::List { ordered, items } => blocks.push(Block::List { ordered, items }),
        WorkingBlock::FencedCode { language, code } => {
            blocks.push(Block::FencedCode { language, code })
        }
        WorkingBlock::Table { headers, rows } => blocks.push(Block::Table { headers, rows }),
    }
}

fn append_text(
    current: &mut Option<WorkingBlock>,
    text: &str,
    style: InlineStyle,
    link: Option<String>,
    table: &mut TableContext,
) {
    if matches!(current, Some(WorkingBlock::Table { .. })) {
        table.current_cell.push(Inline {
            text: text.to_string(),
            style,
            link,
        });
        return;
    }

    match current {
        Some(WorkingBlock::Heading { content, .. })
        | Some(WorkingBlock::Paragraph { content, .. }) => content.push(Inline {
            text: text.to_string(),
            style,
            link,
        }),
        Some(WorkingBlock::List { items, .. }) => {
            if let Some(item) = items.last_mut() {
                item.push(Inline {
                    text: text.to_string(),
                    style,
                    link,
                });
            }
        }
        Some(WorkingBlock::FencedCode { code, .. }) => code.push_str(text),
        Some(WorkingBlock::Table { .. }) => table.current_cell.push(Inline {
            text: text.to_string(),
            style,
            link,
        }),
        None => {}
    }
}

fn push_style<F>(style: &mut InlineStyle, stack: &mut Vec<InlineStyle>, mutate: F)
where
    F: FnOnce(&mut InlineStyle),
{
    stack.push(*style);
    mutate(style);
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn inline_text(content: &[Inline]) -> String {
    content.iter().map(|inline| inline.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::{Block, Document};

    #[test]
    fn builds_outline_and_preserves_styles() {
        let document = Document::parse("# Hello\n\nA **bold** word.");
        assert_eq!(document.outline[0].title, "Hello");
        assert!(matches!(
            document.blocks[0],
            Block::Heading { level: 1, .. }
        ));
        assert!(matches!(document.blocks[1], Block::Paragraph { .. }));
    }

    #[test]
    fn parses_lists_and_code() {
        let document = Document::parse("- One\n- Two\n\n```rust\nlet value = 1;\n```");
        assert!(matches!(document.blocks[0], Block::List { .. }));
        assert!(matches!(document.blocks[1], Block::FencedCode { .. }));
    }

    #[test]
    fn preserves_table_headers_and_rows() {
        let document = Document::parse("| Name | Value |\n| --- | --- |\n| MarkR | reader |");

        let Block::Table { headers, rows } = &document.blocks[0] else {
            panic!("expected table block");
        };
        assert_eq!(headers.len(), 2);
        assert_eq!(rows.len(), 1);
        assert_eq!(super::inline_text(&headers[0]), "Name");
    }

    #[test]
    fn preserves_task_state_in_list_items() {
        let document = Document::parse("- [x] Done\n- [ ] Next");
        let Block::List { items, .. } = &document.blocks[0] else {
            panic!("expected list block");
        };

        assert_eq!(items[0][0].text, "☑ ");
        assert_eq!(items[0][0].style.task, Some(true));
        assert_eq!(items[1][0].style.task, Some(false));
    }

    #[test]
    fn parses_markdown_images_as_image_blocks() {
        let document = Document::parse("Before\n\n![MarkR logo](assets/markr-logo.png)\n\nAfter");

        assert!(matches!(document.blocks[0], Block::Paragraph { .. }));
        let Block::Image { src, alt } = &document.blocks[1] else {
            panic!("expected image block");
        };
        assert_eq!(src, "assets/markr-logo.png");
        assert_eq!(alt, "MarkR logo");
        assert!(matches!(document.blocks[2], Block::Paragraph { .. }));
    }

    #[test]
    fn extracts_images_from_html_blocks_and_skips_wrapper_tags() {
        let document = Document::parse(
            "<p align=\"center\">\n  <img src=\"assets/logo.png\" alt=\"Logo\" width=\"360\">\n</p>",
        );

        assert_eq!(document.blocks.len(), 1);
        let Block::Image { src, alt } = &document.blocks[0] else {
            panic!("expected image block");
        };
        assert_eq!(src, "assets/logo.png");
        assert_eq!(alt, "Logo");
    }

    #[test]
    fn reflows_soft_breaks_and_keeps_hard_ones() {
        // A wrapped source line is not a line break in the prose.
        let document = Document::parse("one\ntwo");
        let Block::Paragraph { content, .. } = &document.blocks[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(super::inline_text(content), "one two");

        // Two trailing spaces are an explicit break and survive.
        let document = Document::parse("one  \ntwo");
        let Block::Paragraph { content, .. } = &document.blocks[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(super::inline_text(content), "one\ntwo");
    }
}
