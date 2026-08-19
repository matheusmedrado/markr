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
    Html(String),
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
                Event::Text(text) => append_text(
                    &mut current,
                    text.as_ref(),
                    style,
                    link.clone(),
                    &mut table_context,
                ),
                Event::Code(text) => append_text(
                    &mut current,
                    text.as_ref(),
                    InlineStyle {
                        code: true,
                        ..InlineStyle::default()
                    },
                    link.clone(),
                    &mut table_context,
                ),
                Event::Html(html) => {
                    if current.is_none() {
                        blocks.push(Block::Html(html.into_string()));
                    } else {
                        append_text(
                            &mut current,
                            html.as_ref(),
                            style,
                            link.clone(),
                            &mut table_context,
                        );
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    append_text(&mut current, "\n", style, link.clone(), &mut table_context);
                }
                Event::Rule => blocks.push(Block::ThematicBreak),
                Event::TaskListMarker(checked) => append_text(
                    &mut current,
                    if checked { "[x] " } else { "[ ] " },
                    style,
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
}
