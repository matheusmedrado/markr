use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub outline: Vec<Heading>,
    /// Where each block in `blocks` came from, by the same index. Private so
    /// the two can only ever be pushed together, through [`Blocks::push`].
    block_sources: Vec<Range<usize>>,
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
    /// The bytes of the source this inline was rendered from.
    ///
    /// For ordinary text this is the text itself, so the markers around it
    /// are the gaps between one inline's range and the next: in
    /// `MarkR **1.2** ships`, the inline `1.2` spans 17..20 and the asterisks
    /// are what is left over. For a few events the source is longer than what
    /// it renders — inline code keeps its backticks, a task marker is written
    /// `[ ]` and drawn as a box — and [`Inline::maps_directly`] says which.
    pub source: Range<usize>,
}

impl Inline {
    // Read by the tests here, and by the source map that layout gains next.
    // The parser is the half that can be built and checked on its own, so it
    // lands first rather than inside a larger change.
    #[allow(dead_code)]
    /// Whether a position inside the rendered text is the same position
    /// inside the source, so the two map through one another directly.
    ///
    /// True when the two run to the same length, which is the case for
    /// ordinary text. It is false wherever the source carries markup the
    /// reader never sees — the backticks around inline code, the `[ ]` of a
    /// task marker, an escape, an entity — and those have to be mapped as
    /// whole units rather than counted into.
    ///
    /// Equal length is not quite equal text: a wrapped source line arrives as
    /// a newline and is drawn as a space. One byte either way, so the
    /// position still lands where it should, which is what this is for.
    pub fn maps_directly(&self) -> bool {
        self.source.len() == self.text.len()
    }
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
    /// The source a block was produced from, markers and fences included: a
    /// heading's range covers its `#`, a fenced block's covers both fences.
    // See the note on `Inline::maps_directly`.
    #[allow(dead_code)]
    pub fn block_source(&self, index: usize) -> Option<Range<usize>> {
        self.block_sources.get(index).cloned()
    }

    pub fn parse(source: &str) -> Self {
        let mut blocks = Blocks::default();
        let mut current: Option<Working> = None;
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

        // `into_offset_iter` pairs every event with the bytes it came from,
        // which is the whole point: an opening tag carries the span of the
        // element it opens, and a text event carries exactly the text.
        for (event, range) in parser.into_offset_iter() {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading(level, _, _) => {
                        current = Some(Working {
                            block: WorkingBlock::Heading {
                                level: heading_level(level),
                                content: Vec::new(),
                            },
                            source: range.clone(),
                        });
                    }
                    Tag::Paragraph if !working_is_list(&current) => {
                        current = Some(Working {
                            block: WorkingBlock::Paragraph {
                                content: Vec::new(),
                                quote_depth,
                            },
                            source: range.clone(),
                        });
                    }
                    Tag::BlockQuote => quote_depth = quote_depth.saturating_add(1),
                    Tag::List(ordered) => {
                        if !working_is_list(&current) {
                            current = Some(Working {
                                block: WorkingBlock::List {
                                    ordered,
                                    items: Vec::new(),
                                },
                                source: range.clone(),
                            });
                        }
                    }
                    Tag::Item => {
                        if let Some(Working {
                            block: WorkingBlock::List { items, .. },
                            ..
                        }) = current.as_mut()
                        {
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
                        current = Some(Working {
                            block: WorkingBlock::FencedCode {
                                language,
                                code: String::new(),
                            },
                            source: range.clone(),
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
                            source: range.clone(),
                        });
                    }
                    Tag::Table(_) => {
                        table_context = TableContext::default();
                        current = Some(Working {
                            block: WorkingBlock::Table {
                                headers: Vec::new(),
                                rows: Vec::new(),
                            },
                            source: range.clone(),
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
                                current.as_ref().map(|working| &working.block),
                                Some(WorkingBlock::Paragraph { content, .. })
                                    if content.len() == work.marker
                            );
                            if replaces_current {
                                current = None;
                            } else {
                                finish_current(&mut current, &mut blocks);
                            }
                            blocks.push(
                                Block::Image {
                                    src: work.src,
                                    alt: work.alt,
                                },
                                work.source,
                            );
                        }
                    }
                    Tag::TableHead => {
                        table_context.in_head = false;
                        if let Some(Working {
                            block: WorkingBlock::Table { headers, .. },
                            ..
                        }) = current.as_mut()
                        {
                            *headers = std::mem::take(&mut table_context.current_row);
                        }
                    }
                    Tag::TableRow => {
                        if let Some(Working {
                            block: WorkingBlock::Table { headers, rows },
                            ..
                        }) = current.as_mut()
                        {
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
                            range.clone(),
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
                            range.clone(),
                            &mut table_context,
                        );
                    }
                }
                Event::Html(html) => {
                    let img_tags = extract_img_tags(html.as_ref());
                    if !img_tags.is_empty() {
                        finish_current(&mut current, &mut blocks);
                        for (src, alt) in img_tags {
                            blocks.push(Block::Image { src, alt }, range.clone());
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
                            range.clone(),
                            &mut table_context,
                        );
                    }
                }
                Event::Rule => blocks.push(Block::ThematicBreak, range.clone()),
                Event::TaskListMarker(checked) => append_text(
                    &mut current,
                    if checked { "☑ " } else { "☐ " },
                    InlineStyle {
                        task: Some(checked),
                        ..style
                    },
                    link.clone(),
                    range.clone(),
                    &mut table_context,
                ),
                Event::FootnoteReference(name) => append_text(
                    &mut current,
                    &format!("[{}]", name),
                    style,
                    link.clone(),
                    range.clone(),
                    &mut table_context,
                ),
            }
        }

        finish_current(&mut current, &mut blocks);
        let outline = blocks
            .blocks
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

        Self {
            blocks: blocks.blocks,
            outline,
            block_sources: blocks.sources,
        }
    }
}

/// Blocks and where each came from, pushed together so the two can never
/// fall out of step.
#[derive(Default)]
struct Blocks {
    blocks: Vec<Block>,
    sources: Vec<Range<usize>>,
}

impl Blocks {
    fn push(&mut self, block: Block, source: Range<usize>) {
        self.blocks.push(block);
        self.sources.push(source);
    }
}

/// A block being built, and the source it was opened on. `pulldown-cmark`
/// hands over the whole span at the opening tag, so this is known up front
/// rather than grown as the block fills.
#[derive(Debug)]
struct Working {
    block: WorkingBlock,
    source: Range<usize>,
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
    source: Range<usize>,
}

fn current_content_len(current: &Option<Working>) -> usize {
    match current.as_ref().map(|working| &working.block) {
        Some(WorkingBlock::Paragraph { content, .. })
        | Some(WorkingBlock::Heading { content, .. }) => content.len(),
        _ => usize::MAX,
    }
}

fn working_is_list(current: &Option<Working>) -> bool {
    matches!(
        current.as_ref().map(|working| &working.block),
        Some(WorkingBlock::List { .. })
    )
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

fn finish_current(current: &mut Option<Working>, blocks: &mut Blocks) {
    let Some(Working { block, source }) = current.take() else {
        return;
    };
    let block = match block {
        WorkingBlock::Heading { level, content } => Block::Heading { level, content },
        WorkingBlock::Paragraph {
            content,
            quote_depth,
        } => Block::Paragraph {
            content,
            quote_depth,
        },
        WorkingBlock::List { ordered, items } => Block::List { ordered, items },
        WorkingBlock::FencedCode { language, code } => Block::FencedCode { language, code },
        WorkingBlock::Table { headers, rows } => Block::Table { headers, rows },
    };
    blocks.push(block, source);
}

fn append_text(
    current: &mut Option<Working>,
    text: &str,
    style: InlineStyle,
    link: Option<String>,
    source: Range<usize>,
    table: &mut TableContext,
) {
    let inline = Inline {
        text: text.to_string(),
        style,
        link,
        source,
    };

    match current.as_mut().map(|working| &mut working.block) {
        Some(WorkingBlock::Table { .. }) => table.current_cell.push(inline),
        Some(WorkingBlock::Heading { content, .. })
        | Some(WorkingBlock::Paragraph { content, .. }) => content.push(inline),
        Some(WorkingBlock::List { items, .. }) => {
            if let Some(item) = items.last_mut() {
                item.push(inline);
            }
        }
        Some(WorkingBlock::FencedCode { code, .. }) => code.push_str(text),
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
    use super::{Block, Document, Inline};

    /// Every inline in the document, in order.
    fn inlines(document: &Document) -> Vec<&Inline> {
        let mut found = Vec::new();
        for block in &document.blocks {
            match block {
                Block::Heading { content, .. } | Block::Paragraph { content, .. } => {
                    found.extend(content)
                }
                Block::List { items, .. } => {
                    for item in items {
                        found.extend(item);
                    }
                }
                Block::Table { headers, rows } => {
                    for cell in headers {
                        found.extend(cell);
                    }
                    for row in rows {
                        for cell in row {
                            found.extend(cell);
                        }
                    }
                }
                Block::FencedCode { .. } | Block::ThematicBreak | Block::Image { .. } => {}
            }
        }
        found
    }

    #[test]
    fn an_inline_source_is_the_text_itself_and_markers_are_the_gaps() {
        let source = "MarkR **1.2** ships";
        let document = Document::parse(source);
        let found = inlines(&document);

        let spans: Vec<_> = found.iter().map(|inline| inline.source.clone()).collect();
        assert_eq!(spans, vec![0..6, 8..11, 13..19]);

        // What is left over between them is exactly the asterisks, which is
        // what lets a renderer hide them and still know where they are.
        assert_eq!(&source[6..8], "**");
        assert_eq!(&source[11..13], "**");
    }

    #[test]
    fn a_transparent_inline_slices_its_own_text_out_of_the_source() {
        let source = "# A *heading*\n\nAnd a [link](./x.md) here.\n";
        let document = Document::parse(source);

        for inline in inlines(&document) {
            if inline.maps_directly() {
                assert_eq!(
                    &source[inline.source.clone()],
                    inline.text,
                    "a directly mapped inline must slice back to its own text"
                );
            }
        }
    }

    #[test]
    fn the_whole_readme_maps_back_to_itself() {
        // The strongest form of the claim: over a real document, every inline
        // that says it maps directly does, and none reaches past the source.
        let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
            .expect("README");
        let document = Document::parse(&source);
        let found = inlines(&document);
        assert!(found.len() > 100, "the README should be substantial");

        for inline in found {
            assert!(
                inline.source.end <= source.len(),
                "an inline reached past the end of the source"
            );
            assert!(inline.source.start <= inline.source.end);
            if inline.maps_directly() {
                let slice = &source[inline.source.clone()];
                // A wrapped source line is a newline drawn as a space: the
                // same length, and the position maps, but the character is
                // normalised on the way through.
                let normalised_break = slice.chars().all(char::is_whitespace)
                    && inline.text.chars().all(char::is_whitespace);
                if !normalised_break {
                    assert_eq!(slice, inline.text);
                }
            }
        }
    }

    #[test]
    fn a_wrapped_source_line_maps_to_the_newline_it_came_from() {
        let source = "one\ntwo";
        let document = Document::parse(source);
        let found = inlines(&document);

        // The paragraph reads as "one two", and the space in the middle is
        // the newline: one byte, mapping to where the wrap actually is.
        let space = found
            .iter()
            .find(|inline| inline.text == " ")
            .expect("the wrapped line reads as a space");
        assert_eq!(&source[space.source.clone()], "\n");
        assert!(space.maps_directly(), "one byte either way still maps");
    }

    #[test]
    fn inline_code_keeps_its_backticks_and_says_it_is_not_direct() {
        let source = "a `code` b";
        let document = Document::parse(source);
        let code = inlines(&document)
            .into_iter()
            .find(|inline| inline.style.code)
            .expect("an inline code span");

        assert_eq!(&source[code.source.clone()], "`code`");
        assert_eq!(code.text, "code");
        assert!(
            !code.maps_directly(),
            "the backticks are in the source but not on screen"
        );
    }

    #[test]
    fn a_task_marker_is_written_in_brackets_and_drawn_as_a_box() {
        let source = "- [x] done";
        let document = Document::parse(source);
        let marker = inlines(&document)
            .into_iter()
            .find(|inline| inline.style.task.is_some())
            .expect("a task marker");

        assert_eq!(&source[marker.source.clone()], "[x]");
        assert_eq!(marker.text, "\u{2611} ");
        assert!(!marker.maps_directly());
    }

    #[test]
    fn a_block_source_covers_the_markers_that_made_it() {
        let source = "# Title\n\n```rust\nfn main() {}\n```\n";
        let document = Document::parse(source);

        let heading = document.block_source(0).expect("a heading source");
        assert_eq!(
            &source[heading], "# Title\n",
            "the hash belongs to the block"
        );

        let code = document.block_source(1).expect("a code source");
        assert_eq!(
            &source[code], "```rust\nfn main() {}\n```",
            "both fences belong to the block"
        );
    }

    #[test]
    fn every_block_has_a_source_and_they_never_fall_out_of_step() {
        let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
            .expect("README");
        let document = Document::parse(&source);

        assert!(!document.blocks.is_empty());
        for index in 0..document.blocks.len() {
            let span = document
                .block_source(index)
                .unwrap_or_else(|| panic!("block {index} has no source"));
            assert!(span.end <= source.len());
            assert!(span.start <= span.end);
        }
        assert!(
            document.block_source(document.blocks.len()).is_none(),
            "there is no source past the last block"
        );
    }

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
