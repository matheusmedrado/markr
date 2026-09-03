use std::ops::Range;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block as TuiBlock, Clear, List, ListItem, ListState, Padding, Paragraph};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus, ResponsiveMode, SidebarPanel};
use crate::explorer::EntryKind;
use crate::images::Asset;
use crate::layout;
use crate::selection;
use crate::theme::Theme;

/// Blends `color` toward `background`, keeping `keep` of the original. Dimming
/// toward the ground rather than toward black keeps light palettes right, and
/// keeps every cell's own hue instead of flattening the page to one grey.
fn toward_background(color: Color, background: Color, keep: f32) -> Color {
    let (Color::Rgb(red, green, blue), Color::Rgb(ground_red, ground_green, ground_blue)) =
        (color, background)
    else {
        return color;
    };
    let blend = |value: u8, ground: u8| {
        (f32::from(value) * keep + f32::from(ground) * (1.0 - keep)).round() as u8
    };
    Color::Rgb(
        blend(red, ground_red),
        blend(green, ground_green),
        blend(blue, ground_blue),
    )
}

/// Redraws everything already painted in `area` out of focus, for the document
/// behind an overlay.
fn dim_area(frame: &mut Frame, area: Rect, theme: Theme) {
    const KEEP: f32 = 0.38;
    let buffer = frame.buffer_mut();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let cell = &mut buffer[(x, y)];
            let fg = toward_background(cell.fg, theme.background, KEEP);
            let bg = toward_background(cell.bg, theme.background, KEEP);
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
    }
}

/// Overlays are frameless: a filled block on `surface_active`, lifted off the
/// dimmed document by value alone.
fn render_overlay_panel(frame: &mut Frame, area: Rect, theme: Theme) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        TuiBlock::default().style(Style::default().bg(theme.surface_active)),
        area,
    );
}

fn overlay_inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(1),
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    )
}

fn fill_area(frame: &mut Frame, area: Rect, background: Color) {
    frame.render_widget(
        TuiBlock::default().style(Style::default().bg(background)),
        area,
    );
}

pub fn render(frame: &mut Frame, app: &App) {
    // Nothing paints a background: the terminal's own ground shows through, so
    // transparent and blurred terminals keep working. Fills are reserved for
    // things that genuinely need to occlude — slabs, chips and overlays.

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_header(frame, app, vertical[0]);
    render_body(frame, app, vertical[2]);
    render_status(frame, app, vertical[3]);

    if app.help_visible || app.help_progress(Instant::now()) > 0.0 {
        render_help(frame, app);
    }
    if app.has_unsaved_prompt() {
        render_unsaved_prompt(frame, app);
    }
    if app.has_external_prompt() {
        render_external_prompt(frame, app);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let quiet = Style::default().fg(theme.reader_border);
    // `display_name` is already the path relative to the workspace root, so the
    // file name is split off it rather than appended a second time.
    let display = app.workspace.display_name();
    let (parent, name) = match display.rfind('/') {
        Some(index) => (
            display[..=index].to_string(),
            display[index + 1..].to_string(),
        ),
        None => (String::new(), display),
    };
    let mut spans = vec![
        Span::raw("  "),
        Span::styled("M A R K R", theme.accent()),
        Span::styled("   ·   ", quiet),
    ];
    if !parent.is_empty() {
        spans.push(Span::styled(
            parent,
            Style::default().fg(theme.chrome_muted),
        ));
    }
    spans.push(Span::styled(name, Style::default().fg(theme.chrome_text)));
    if app.is_editing() && app.editor_dirty() {
        spans.push(Span::styled("  ●", Style::default().fg(theme.accent)));
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(12)])
        .split(area);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(format!("{}  ", theme.name), quiet))
            .alignment(Alignment::Right),
        chunks[1],
    );
}

fn active_file_name(app: &App) -> Option<String> {
    app.workspace
        .active_path()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
}

fn render_body(frame: &mut Frame, app: &App, area: Rect) {
    let now = Instant::now();
    if app.responsive_mode() == ResponsiveMode::Fullscreen && app.sidebar_visible {
        fill_area(frame, area, app.theme.background);
        render_sidebar(frame, app, area);
        return;
    }

    let document_area = reader_area(app, area);
    if document_area.width > 0 && document_area.height > 0 {
        render_reader(frame, app, document_area);
    }

    if app.sidebar_visible || app.sidebar_progress(now) > 0.0 {
        let sidebar_area = match app.responsive_mode() {
            ResponsiveMode::Attached => Rect::new(
                area.x,
                area.y,
                app.sidebar_width().min(area.width),
                area.height,
            ),
            ResponsiveMode::Overlay => {
                let width = ((f32::from(app.sidebar_width()) * app.sidebar_progress(now)) as u16)
                    .max(1)
                    .min(area.width);
                Rect::new(area.x, area.y, width, area.height)
            }
            ResponsiveMode::Fullscreen => area,
        };
        if app.responsive_mode() != ResponsiveMode::Attached {
            // The sidebar is modal at these widths: push the document behind it
            // out of focus rather than letting it be cut mid-word.
            dim_area(frame, document_area, app.theme);
        }
        render_sidebar(frame, app, sidebar_area);
    }
}

fn reader_area(app: &App, area: Rect) -> Rect {
    if app.responsive_mode() == ResponsiveMode::Fullscreen && app.sidebar_visible {
        return Rect::default();
    }
    // The measure carries its own gutter and right pad, so the reader needs no
    // outer margin of its own.
    let x = if app.responsive_mode() == ResponsiveMode::Attached && app.sidebar_visible {
        area.x.saturating_add(app.sidebar_width())
    } else {
        area.x
    };
    let width = area.width.saturating_sub(x.saturating_sub(area.x));
    Rect::new(x, area.y, width, area.height)
}

fn render_reader(frame: &mut Frame, app: &App, document_area: Rect) {
    let theme = app.theme;
    if app.is_editing() {
        render_editor(frame, app, layout::editor_inner(document_area));
        return;
    }
    let inner = layout::reader_inner(document_area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let start = app.scroll.min(app.document_layout.lines.len());
    let end = start.saturating_add(usize::from(inner.height));
    let selection_range = app
        .selection
        .as_ref()
        .map(|selection| selection.normalized());
    let visible_lines = app.document_layout.lines[start..end.min(app.document_layout.lines.len())]
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            let line_index = start + offset;
            let highlights = app
                .search_matches_on_line(line_index)
                .map(|(index, search_match)| {
                    (
                        search_match.range.clone(),
                        app.selected_search_match() == Some(index),
                    )
                })
                .collect::<Vec<_>>();
            // Scrolling redraws every visible line. With nothing to highlight —
            // the common case — the spans are borrowed straight out of the
            // layout rather than cloning every string in the viewport.
            if highlights.is_empty() && selection_range.is_none() {
                return borrowed_line(line);
            }
            let highlighted = highlight_search_line(line, &highlights, theme);
            if let Some((sel_start, sel_end)) = selection_range {
                highlight_selection_line(
                    &highlighted,
                    line_index,
                    &sel_start,
                    &sel_end,
                    app.document_layout.content_margin,
                    theme,
                )
            } else {
                highlighted
            }
        })
        .collect::<Vec<_>>();
    let paragraph =
        Paragraph::new(Text::from(visible_lines)).style(Style::default().fg(theme.text));
    frame.render_widget(paragraph, inner);

    let content_margin = app.document_layout.content_margin;
    let content_width = layout::measure_for(inner.width);
    for region in &app.document_layout.image_regions {
        let Some(Asset::Ready {
            protocol,
            cols,
            rows,
        }) = app.images.asset(&region.src)
        else {
            continue;
        };
        if let Some(position) = image_position(
            inner,
            content_margin,
            content_width,
            region.line,
            app.scroll,
            *cols,
            *rows,
        ) {
            frame.render_widget(SlicedImage::new(protocol, position), inner);
        }
    }

    render_reading_rail(frame, app, document_area);
}

/// One column at the reader's right edge showing how far through the document
/// the viewport sits. It doubles as the reader's focus indicator, which is why
/// the reader itself needs no frame.
fn render_reading_rail(frame: &mut Frame, app: &App, document_area: Rect) {
    let height = usize::from(document_area.height);
    let total = app.document_layout.lines.len();
    if document_area.width == 0 || height == 0 || total <= height {
        return;
    }

    let thumb = height
        .saturating_mul(height)
        .saturating_div(total)
        .clamp(1, height);
    let travel = height.saturating_sub(thumb);
    let span = total.saturating_sub(height).max(1);
    let start = app.scroll.min(span).saturating_mul(travel) / span;

    let color = if app.focus == Focus::Document {
        app.theme.accent
    } else {
        app.theme.accent_soft
    };
    let x = document_area.right().saturating_sub(1);
    let buffer = frame.buffer_mut();
    for offset in 0..thumb {
        let Ok(row) = u16::try_from(start.saturating_add(offset)) else {
            break;
        };
        let y = document_area.y.saturating_add(row);
        if y >= document_area.bottom() {
            break;
        }
        let cell = &mut buffer[(x, y)];
        cell.set_symbol("▐");
        cell.set_fg(color);
    }
}

/// A view of a laid out line that borrows its text instead of copying it.
fn borrowed_line<'a>(line: &'a Line<'static>) -> Line<'a> {
    Line::from(
        line.spans
            .iter()
            .map(|span| Span::styled(span.content.as_ref(), span.style))
            .collect::<Vec<_>>(),
    )
}

fn render_editor(frame: &mut Frame, app: &App, inner: Rect) {
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = app.theme;
    let lines = app.editor_lines();
    let cursor = app.editor_cursor();
    let highlighted_lines = app.editor_highlight();
    let wrap = app.editor_wrap();
    let line_number_width = lines.len().max(1).to_string().len();
    let gutter_width = app.editor_gutter_width();

    // Which visual row the caret is on, so its line keeps the lit number and
    // the caret itself can be placed without locating it a second time.
    let caret = cursor.map(|cursor| wrap.locate(cursor, lines));
    let start = app.editor_scroll.min(wrap.len());

    let text = wrap
        .rows()
        .iter()
        .enumerate()
        .skip(start)
        .take(usize::from(inner.height))
        .map(|(index, row)| {
            let on_caret_line = caret.is_some_and(|(caret_row, _)| {
                wrap.rows()
                    .get(caret_row)
                    .is_some_and(|caret| caret.line == row.line)
            });
            let number_style = if on_caret_line {
                theme.accent()
            } else {
                theme.muted()
            };

            // Only the row that opens a source line carries its number; the
            // rows it wraps onto leave the gutter blank so the column of
            // numbers still reads as one number per line of the file.
            let prefix = if row.opens_line() {
                format!(
                    "  {:>line_number_width$} │ ",
                    row.line + 1,
                    line_number_width = line_number_width
                )
            } else {
                format!(
                    "  {:>line_number_width$}   ",
                    "",
                    line_number_width = line_number_width
                )
            };

            let line = lines.get(row.line);
            let (from, to) = line
                .map(|line| (cell_offset(line, row.start), cell_offset(line, row.end)))
                .unwrap_or((0, 0));
            let content = highlighted_lines
                .get(row.line)
                .map(|spans| slice_spans_by_columns(spans, from, to))
                .unwrap_or_else(|| {
                    vec![Span::styled(
                        line.map(|line| selection::slice_by_columns(line, from, to))
                            .unwrap_or_default(),
                        Style::default().fg(theme.text),
                    )]
                });

            let mut spans = vec![Span::styled(prefix, number_style)];
            spans.extend(content);
            let _ = index;
            Line::from(spans)
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(Text::from(text)).style(Style::default().fg(theme.text)),
        inner,
    );

    let Some((caret_row, caret_column)) = caret else {
        return;
    };
    if caret_row < start || caret_row >= start.saturating_add(usize::from(inner.height)) {
        return;
    }
    let x = inner
        .x
        .saturating_add(gutter_width as u16)
        .saturating_add(caret_column as u16)
        .min(inner.right().saturating_sub(1));
    let y = inner
        .y
        .saturating_add(caret_row.saturating_sub(start) as u16);
    frame.set_cursor_position(Position::new(x, y));
}

/// How many terminal cells of `line` sit before grapheme column `column`.
fn cell_offset(line: &str, column: usize) -> usize {
    let byte = line
        .grapheme_indices(true)
        .nth(column)
        .map(|(index, _)| index)
        .unwrap_or(line.len());
    UnicodeWidthStr::width(&line[..byte])
}

fn slice_spans_by_columns(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut column: usize = 0;

    for span in spans {
        let mut segment = String::new();
        let style = span.style;
        for grapheme in span.content.graphemes(true) {
            let grapheme_start = column;
            let grapheme_end = column.saturating_add(grapheme.width());
            if grapheme_end > grapheme_start && grapheme_start < end && grapheme_end > start {
                segment.push_str(grapheme);
            }
            column = grapheme_end;
            if column >= end {
                break;
            }
        }
        if !segment.is_empty() {
            result.push(Span::styled(segment, style));
        }
        if column >= end {
            break;
        }
    }

    result
}

fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = app.theme;
    // Attached, the sidebar shares the reader's single plane. Overlaid it has to
    // occlude: clearing first drops the reader's glyphs, which a background fill
    // on its own would leave showing through.
    if app.responsive_mode() != ResponsiveMode::Attached {
        frame.render_widget(Clear, area);
        fill_area(frame, area, theme.surface);
    }

    // One hairline column stands in for the panel frame, and carries focus.
    let hairline = if app.focus == Focus::Sidebar {
        theme.accent_soft
    } else {
        theme.reader_border
    };
    let hairline_x = area.right().saturating_sub(1);
    {
        let buffer = frame.buffer_mut();
        for y in area.y..area.bottom() {
            let cell = &mut buffer[(hairline_x, y)];
            cell.set_symbol("│");
            cell.set_fg(hairline);
        }
    }

    let inner = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if inner.width == 0 || inner.height < 4 {
        return;
    }

    render_sidebar_tabs(
        frame,
        app,
        Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
    );

    let list_area = Rect::new(
        inner.x,
        inner.y.saturating_add(3),
        inner.width,
        inner.height.saturating_sub(3),
    );
    match app.sidebar_panel {
        SidebarPanel::Outline => render_outline(frame, app, list_area),
        SidebarPanel::Files => render_files(frame, app, list_area),
    }
}

fn render_sidebar_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let (active, inactive) = match app.sidebar_panel {
        SidebarPanel::Outline => ("HEADINGS", "FILES"),
        SidebarPanel::Files => ("FILES", "HEADINGS"),
    };
    let tick = if app.focus == Focus::Sidebar {
        theme.accent
    } else {
        theme.reader_border
    };
    let tabs = Line::from(vec![
        Span::raw(" "),
        Span::styled("▌", Style::default().fg(tick)),
        Span::raw(" "),
        Span::styled(
            active,
            Style::default()
                .fg(theme.chrome_text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("   {inactive}"), Style::default().fg(theme.border)),
    ]);
    frame.render_widget(Paragraph::new(tabs), area);
}

/// The marker column doubles as the sidebar's selection cue, so unselected rows
/// carry nothing and let indentation show the level.
fn sidebar_row(
    selected: bool,
    marker: Option<(&'static str, Color)>,
    text: String,
    style: Style,
    theme: Theme,
    focused: bool,
) -> ListItem<'static> {
    let marker = if selected {
        Span::styled(
            "▌",
            Style::default().fg(if focused {
                theme.accent
            } else {
                theme.accent_soft
            }),
        )
    } else {
        match marker {
            Some((symbol, color)) => Span::styled(symbol, Style::default().fg(color)),
            None => Span::raw(" "),
        }
    };
    ListItem::new(Line::from(vec![
        Span::raw("  "),
        marker,
        Span::raw(" "),
        Span::styled(text, style),
    ]))
}

fn render_outline(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let focused = app.focus == Focus::Sidebar;
    let items: Vec<ListItem> = if app.document.outline.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "    No headings",
            Style::default().fg(theme.border),
        )))]
    } else {
        app.document
            .outline
            .iter()
            .enumerate()
            .map(|(index, heading)| {
                let selected = index == app.outline_selected;
                let indent = "  ".repeat(heading.level.saturating_sub(1) as usize);
                let style = if selected {
                    Style::default().fg(theme.accent)
                } else if heading.level >= 3 {
                    Style::default().fg(theme.border)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                sidebar_row(
                    selected,
                    None,
                    format!("{indent}{}", heading.title),
                    style,
                    theme,
                    focused,
                )
            })
            .collect()
    };

    let list = List::new(items).highlight_style(Style::default());
    let mut state = ListState::default()
        .with_selected((!app.document.outline.is_empty()).then_some(app.outline_selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_files(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let focused = app.focus == Focus::Sidebar;
    let items: Vec<ListItem> = if app.file_explorer.entries().is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "    Empty directory",
            Style::default().fg(theme.border),
        )))]
    } else {
        app.file_explorer
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let active = app.workspace.active_path() == Some(entry.path.as_path());
                let selected = index == app.file_explorer.selected();
                let marker = match entry.kind {
                    EntryKind::Parent => ("▴", theme.border),
                    EntryKind::Directory => ("▸", theme.border),
                    EntryKind::Markdown if active => ("●", theme.accent),
                    EntryKind::Markdown => ("◇", theme.text_muted),
                    EntryKind::File => ("·", theme.reader_border),
                };
                let style = if active {
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else if matches!(entry.kind, EntryKind::File) {
                    Style::default().fg(theme.reader_border)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                let suffix = matches!(entry.kind, EntryKind::Directory)
                    .then_some("/")
                    .unwrap_or_default();
                sidebar_row(
                    selected,
                    Some(marker),
                    format!("{}{}", entry.name, suffix),
                    style,
                    theme,
                    focused,
                )
            })
            .collect()
    };

    let list = List::new(items).highlight_style(Style::default());
    let mut state = ListState::default().with_selected(
        (!app.file_explorer.entries().is_empty()).then_some(app.file_explorer.selected()),
    );
    frame.render_stateful_widget(list, area, &mut state);
}

fn highlight_search_line(
    line: &Line<'static>,
    highlights: &[(Range<usize>, bool)],
    theme: crate::theme::Theme,
) -> Line<'static> {
    if highlights.is_empty() {
        return line.clone();
    }

    let mut spans = Vec::new();
    let mut line_offset = 0;
    for span in &line.spans {
        let content = span.content.as_ref();
        let span_start = line_offset;
        let span_end = span_start + content.len();
        let mut boundaries = vec![span_start, span_end];
        for (range, _) in highlights {
            if range.start < span_end && span_start < range.end {
                boundaries.push(range.start.max(span_start));
                boundaries.push(range.end.min(span_end));
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        for segment in boundaries.windows(2) {
            let start = segment[0];
            let end = segment[1];
            if start == end {
                continue;
            }
            let mut style = span.style;
            if let Some((_, active)) = highlights
                .iter()
                .find(|(range, _)| range.start <= start && end <= range.end)
            {
                let highlight = if *active {
                    Style::default()
                        .fg(theme.background)
                        .bg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .bg(theme.surface_active)
                        .add_modifier(Modifier::UNDERLINED)
                };
                style = style.patch(highlight);
            }
            spans.push(Span::styled(
                content[start - span_start..end - span_start].to_string(),
                style,
            ));
        }
        line_offset = span_end;
    }
    Line::from(spans)
}

fn highlight_selection_line(
    line: &Line<'static>,
    line_index: usize,
    start: &selection::CursorPosition,
    end: &selection::CursorPosition,
    content_margin: usize,
    theme: Theme,
) -> Line<'static> {
    if line_index < start.line || line_index > end.line {
        return line.clone();
    }

    let style = selection::selection_style(theme);
    let start_col = if line_index == start.line {
        content_margin.saturating_add(start.column)
    } else {
        content_margin
    };
    let end_col = if line_index == end.line {
        content_margin.saturating_add(end.column)
    } else {
        usize::MAX
    };

    if start_col >= end_col {
        return line.clone();
    }

    let mut spans = Vec::new();
    let mut column = 0;
    for span in &line.spans {
        let content = span.content.as_ref();
        let mut segment = String::new();
        let mut segment_style = None;
        for grapheme in content.graphemes(true) {
            let grapheme_start = column;
            let grapheme_end = column + grapheme.width();
            let selected = grapheme_end > grapheme_start
                && grapheme_start < end_col
                && grapheme_end > start_col;
            let grapheme_style = if selected {
                span.style.patch(style)
            } else {
                span.style
            };

            if segment_style != Some(grapheme_style) {
                if !segment.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut segment),
                        segment_style.unwrap(),
                    ));
                }
                segment_style = Some(grapheme_style);
            }
            segment.push_str(grapheme);
            column = grapheme_end;
        }
        if !segment.is_empty() {
            spans.push(Span::styled(segment, segment_style.unwrap()));
        }
    }
    Line::from(spans)
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    // The accent is spent on state, not on every hint: keys read as plain text
    // and their labels step back.
    let key = Style::default().fg(theme.chrome_text);
    let label = Style::default().fg(theme.border);

    let left = if let Some(input) = &app.search_input {
        Line::from(vec![
            Span::styled(format!("/{input}"), theme.accent()),
            Span::styled("   Enter search · Esc cancel", label),
        ])
    } else if let Some(message) = app.temporary_message(Instant::now()) {
        Line::from(Span::styled(message, theme.accent()))
    } else if app.is_editing() {
        let mut spans = Vec::new();
        if app.external_change_detected {
            spans.push(Span::styled(
                "⚠ file changed on disk   ",
                Style::default().fg(theme.warning),
            ));
        }
        spans.extend([
            Span::styled("EDIT", theme.accent()),
            Span::styled(
                if app.editor_dirty() {
                    "   unsaved   "
                } else {
                    "   saved   "
                },
                label,
            ),
            Span::styled("^S", key),
            Span::styled(" save   ", label),
            Span::styled("^Z ^Y", key),
            Span::styled(" undo/redo   ", label),
            Span::styled("Esc", key),
            Span::styled(" leave", label),
        ]);
        Line::from(spans)
    } else if let Some((current, total)) = app.search_result_position() {
        Line::from(vec![
            Span::styled(format!("/{}", app.search_query), theme.accent()),
            Span::styled(format!("   {current}/{total}   "), label),
            Span::styled("n", key),
            Span::styled(" next   ", label),
            Span::styled("N", key),
            Span::styled(" previous", label),
        ])
    } else if !app.search_query.is_empty() {
        Line::from(vec![
            Span::styled(format!("/{}", app.search_query), theme.accent()),
            Span::styled("   no matches   ", label),
            Span::styled("/", key),
            Span::styled(" edit", label),
        ])
    } else if app.focus == Focus::Sidebar && app.sidebar_panel == SidebarPanel::Files {
        Line::from(vec![
            Span::styled("h/⌫", key),
            Span::styled(" parent   ", label),
            Span::styled("l/Enter", key),
            Span::styled(" open   ", label),
            Span::styled("r", key),
            Span::styled(" refresh   ", label),
            Span::styled("TAB", key),
            Span::styled(" focus   ", label),
            Span::styled("e", key),
            Span::styled(" edit", label),
        ])
    } else {
        Line::from(vec![
            Span::styled("/", key),
            Span::styled(" search   ", label),
            Span::styled("TAB", key),
            Span::styled(" focus   ", label),
            Span::styled("e", key),
            Span::styled(" edit   ", label),
            Span::styled("?", key),
            Span::styled(" help", label),
        ])
    };

    let file = active_file_name(app).unwrap_or_else(|| "stdin".to_string());
    let right = if let Some(cursor) = app.editor_cursor() {
        format!(
            "{file}  ·  ln {}, col {}  ",
            cursor.line + 1,
            cursor.column + 1
        )
    } else {
        format!("{file}  ·  {}%  ", app.reading_progress())
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(left).block(TuiBlock::default().padding(Padding::left(2))),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(right, label)).alignment(Alignment::Right),
        chunks[1],
    );
}

/// Two columns of shortcuts, grouped, instead of one flat list.
const HELP_KEY_WIDTH: usize = 11;
const HELP_COLUMN_WIDTH: usize = 30;

fn help_cell(
    key: &str,
    description: &str,
    theme: Theme,
    spans: &mut Vec<Span<'static>>,
    pad: bool,
) {
    let used = HELP_KEY_WIDTH.max(key.chars().count()) + description.chars().count();
    spans.push(Span::styled(
        key.to_string(),
        Style::default().fg(theme.chrome_text),
    ));
    spans.push(Span::raw(
        " ".repeat(HELP_KEY_WIDTH.saturating_sub(key.chars().count())),
    ));
    spans.push(Span::styled(
        description.to_string(),
        Style::default().fg(theme.text_muted),
    ));
    if pad {
        spans.push(Span::raw(
            " ".repeat(HELP_COLUMN_WIDTH.saturating_sub(used)),
        ));
    }
}

fn help_row(left: (&str, &str), right: (&str, &str), theme: Theme) -> Line<'static> {
    let mut spans = Vec::new();
    help_cell(left.0, left.1, theme, &mut spans, true);
    help_cell(right.0, right.1, theme, &mut spans, false);
    Line::from(spans)
}

fn help_section(left: &str, right: &str, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{left:<HELP_COLUMN_WIDTH$}"),
            Style::default().fg(theme.accent_soft),
        ),
        Span::styled(right.to_string(), Style::default().fg(theme.accent_soft)),
    ])
}

fn overlay_title(title: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("▌", Style::default().fg(color)),
        Span::styled(
            format!(" {title}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn centered_fixed(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}

fn render_overlay(frame: &mut Frame, theme: Theme, width: u16, height: u16, text: Text<'static>) {
    let full = frame.area();
    let area = centered_fixed(full, width, height);
    dim_area(frame, full, theme);
    render_overlay_panel(frame, area, theme);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(theme.text).bg(theme.surface_active)),
        overlay_inner(area),
    );
}

fn render_help(frame: &mut Frame, app: &App) {
    let theme = app.theme;
    let lines = vec![
        overlay_title("MARKR / QUICK GUIDE", theme.accent),
        Line::default(),
        help_section("READ", "EDIT", theme),
        help_row(("↑ ↓  j k", "move"), ("e", "edit this file"), theme),
        help_row(("g  G", "top / bottom"), ("^S", "save"), theme),
        help_row(
            ("^U  ^D", "page up / down"),
            ("^Z  ^Y", "undo / redo"),
            theme,
        ),
        help_row(("[  ]", "prev / next doc"), ("Esc", "leave editor"), theme),
        Line::default(),
        help_section("NAVIGATE", "SELECT", theme),
        help_row(("Tab", "switch focus"), ("v", "start selection"), theme),
        help_row(("t", "toggle outline"), ("h j k l", "extend"), theme),
        help_row(("1  2", "outline / files"), ("y", "copy"), theme),
        help_row(
            ("Enter", "open selection"),
            ("drag", "select with mouse"),
            theme,
        ),
        Line::default(),
        help_section("FIND", "APPEARANCE", theme),
        help_row(("/", "search the text"), ("T", "cycle theme"), theme),
        help_row(("n  N", "next / previous"), ("q", "quit"), theme),
        Line::default(),
        Line::from(Span::styled(
            "Press any key to return",
            Style::default().fg(theme.border),
        )),
    ];

    // The panel is always its full size: animating the height clipped the last
    // group off the bottom. The reveal is the scrim behind it.
    let height = lines.len() as u16 + 2;
    render_overlay(frame, theme, 64, height, Text::from(lines));
}

fn render_unsaved_prompt(frame: &mut Frame, app: &App) {
    let theme = app.theme;
    let label = Style::default().fg(theme.border);
    let key = Style::default().fg(theme.chrome_text);
    let text = Text::from(vec![
        overlay_title("MARKR / UNSAVED CHANGES", theme.accent),
        Line::default(),
        Line::from("Leave the editor with unsaved changes?"),
        Line::default(),
        Line::from(vec![
            Span::styled("s / Enter", key),
            Span::styled(" save   ", label),
            Span::styled("d", key),
            Span::styled(" discard   ", label),
            Span::styled("Esc", key),
            Span::styled(" continue editing", label),
        ]),
    ]);
    render_overlay(frame, theme, 56, 7, text);
}

fn render_external_prompt(frame: &mut Frame, app: &App) {
    let theme = app.theme;
    let label = Style::default().fg(theme.border);
    let key = Style::default().fg(theme.chrome_text);
    let text = Text::from(vec![
        overlay_title("MARKR / FILE CHANGED", theme.warning),
        Line::default(),
        Line::from("This file changed outside MarkR while editing."),
        Line::from("Choose how to continue:"),
        Line::default(),
        Line::from(vec![
            Span::styled("o / Enter", key),
            Span::styled(" overwrite   ", label),
            Span::styled("r", key),
            Span::styled(" reload   ", label),
            Span::styled("Esc", key),
            Span::styled(" back", label),
        ]),
    ]);
    render_overlay(frame, theme, 56, 8, text);
}

fn image_position(
    inner: Rect,
    content_margin: usize,
    content_width: usize,
    image_line: usize,
    scroll: usize,
    image_width: u16,
    image_height: u16,
) -> Option<SignedPosition> {
    let viewport_start = scroll;
    let viewport_end = scroll.saturating_add(usize::from(inner.height));
    let image_end = image_line.saturating_add(usize::from(image_height));
    if image_line >= viewport_end || image_end <= viewport_start {
        return None;
    }

    // Images align with the measure, not with the whole reader, so they sit
    // under the text rather than under the gutter.
    let content_width = content_width.min(usize::from(inner.width)) as u16;
    let width = image_width.min(content_width);
    let content_x = u16::try_from(content_margin).unwrap_or(u16::MAX);
    let x = content_x.saturating_add((content_width.saturating_sub(width)) / 2);
    let y = if image_line >= scroll {
        image_line.saturating_sub(scroll).min(i16::MAX as usize) as i16
    } else {
        -(scroll.saturating_sub(image_line).min(i16::MAX as usize) as i16)
    };
    Some((x.min(i16::MAX as u16) as i16, y).into())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui_image::picker::{Picker, ProtocolType};

    use super::{highlight_search_line, highlight_selection_line, image_position};
    use crate::app::{App, Message};
    use crate::images::Asset;
    use crate::selection::CursorPosition;
    use crate::theme::Theme;
    use crate::workspace::Workspace;

    #[test]
    fn centers_images_inside_the_content_column_not_the_reader() {
        // A 100 column reader keeps 4 of gutter and 2 of pad, so the measure
        // starts at column 7 and an image centres inside it, clear of the tick.
        let position = image_position(Rect::new(10, 5, 100, 20), 7, 88, 4, 0, 40, 10).unwrap();

        assert_eq!(position.x, 31);
        assert_eq!(position.y, 4);
    }

    #[test]
    fn keeps_partially_visible_images_on_screen_after_scroll() {
        let position = image_position(Rect::new(10, 5, 100, 20), 7, 88, 4, 8, 40, 10).unwrap();

        assert_eq!(position.y, -4);
    }

    #[test]
    fn highlights_all_matches_and_emphasizes_the_selected_one() {
        let theme = Theme::default();
        let line = Line::from(vec![
            Span::styled("Mark", Style::default().fg(theme.link)),
            Span::styled("R reader", Style::default().fg(theme.text)),
        ]);

        let highlighted = highlight_search_line(&line, &[(0..5, false), (6..12, true)], theme);
        let markr = highlighted
            .spans
            .iter()
            .filter(|span| matches!(span.content.as_ref(), "Mark" | "R"))
            .collect::<Vec<_>>();
        let reader = highlighted
            .spans
            .iter()
            .find(|span| span.content == "reader")
            .expect("selected match");

        assert!(markr.iter().all(|span| {
            span.style.bg == Some(theme.surface_active)
                && span.style.add_modifier.contains(Modifier::UNDERLINED)
        }));
        assert_eq!(reader.style.bg, Some(theme.accent));
        assert_eq!(reader.style.fg, Some(theme.background));
        assert!(reader.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn highlights_unicode_selection_without_splitting_utf8() {
        let theme = Theme::default();
        let line = Line::from(vec![
            Span::styled("aé", Style::default().fg(theme.link)),
            Span::styled("界b", Style::default().fg(theme.text)),
        ]);

        let highlighted = highlight_selection_line(
            &line,
            0,
            &CursorPosition::new(0, 1),
            &CursorPosition::new(0, 4),
            0,
            theme,
        );

        assert_eq!(highlighted.to_string(), "aé界b");
        assert_eq!(highlighted.spans[1].content, "é");
        assert_eq!(highlighted.spans[1].style.bg, Some(theme.accent));
        assert_eq!(highlighted.spans[2].content, "界");
        assert_eq!(highlighted.spans[2].style.bg, Some(theme.accent));
    }

    #[test]
    fn the_reader_and_the_sidebar_carry_no_frame() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render reader");

        let buffer = terminal.backend().buffer();
        let symbols = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        for glyph in [
            "╭", "╮", "╰", "╯", "┌", "┐", "└", "┘", "├", "┤", "┬", "┴", "┼",
        ] {
            assert!(
                !symbols.contains(glyph),
                "`{glyph}` still frames part of the reader"
            );
        }

        // One hairline column stands in for the sidebar's panel border.
        let hairline_x = app.sidebar_width().saturating_sub(1);
        assert_eq!(buffer[(hairline_x, 6)].symbol(), "│");
        assert_eq!(buffer[(hairline_x, 6)].fg, Theme::default().reader_border);
    }

    #[test]
    fn the_shell_paints_one_plane_with_no_panel_frames() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render shell");

        let buffer = terminal.backend().buffer();
        let theme = Theme::default();
        let header = (0..120).fold(String::new(), |mut text, x| {
            text.push_str(buffer[(x, 0)].symbol());
            text
        });
        assert!(!header.contains("1/1"));
        assert!(header.contains("M A R K R"));

        // Row 1 is the breathing row between the header and the body.
        assert_eq!(buffer[(0, 1)].symbol(), " ");

        // Nothing paints a ground of its own, so a transparent or blurred
        // terminal shows through the header, the sidebar and the reader alike.
        for cell in [(0, 0), (118, 10), (5, 10), (60, 30)] {
            assert_eq!(
                buffer[cell].bg,
                Color::Reset,
                "the shell painted over the terminal at {cell:?}"
            );
        }

        // A single hairline column stands in for the sidebar's frame.
        assert_eq!(buffer[(29, 5)].symbol(), "│");
        assert_eq!(buffer[(29, 5)].fg, theme.reader_border);
    }

    #[test]
    fn the_sidebar_marks_its_active_panel_with_a_tick_not_an_underline() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render shell");

        let buffer = terminal.backend().buffer();
        let theme = Theme::default();
        let row = |y: u16| {
            (0..29).fold(String::new(), |mut text, x| {
                text.push_str(buffer[(x, y)].symbol());
                text
            })
        };

        let tabs = row(3);
        assert!(tabs.contains("HEADINGS"));
        assert!(tabs.contains("FILES"));
        assert_eq!(buffer[(1, 3)].symbol(), "▌");
        assert_eq!(buffer[(1, 3)].fg, theme.reader_border);

        // The underline the tick replaced is gone.
        assert!(!row(4).contains('─'));

        // Unselected outline rows carry no marker: indentation shows the level.
        assert_eq!(buffer[(2, 5)].symbol(), "▌");
        assert_eq!(buffer[(2, 5)].fg, theme.accent_soft);
        assert_eq!(buffer[(2, 6)].symbol(), " ");
    }

    #[test]
    fn focus_moves_the_tick_the_hairline_and_the_rail_to_the_accent() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");

        // Reading: the rail carries focus, the sidebar recedes.
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render reader focus");
        let theme = Theme::default();
        {
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(119, 2)].symbol(), "▐");
            assert_eq!(buffer[(119, 2)].fg, theme.accent);
            assert_eq!(buffer[(29, 5)].fg, theme.reader_border);
        }

        app.update(Message::Key {
            key: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render sidebar focus");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(1, 3)].fg, theme.accent);
        assert_eq!(buffer[(29, 5)].fg, theme.accent_soft);
        assert_eq!(buffer[(119, 2)].fg, theme.accent_soft);
    }

    #[test]
    fn the_overlaid_sidebar_occludes_the_reader_instead_of_letting_it_through() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        // Between 72 and 99 columns the sidebar is drawn over the reader.
        app.update(Message::Resize {
            width: 84,
            height: 40,
            at: Instant::now(),
        });
        assert_eq!(app.responsive_mode(), crate::app::ResponsiveMode::Overlay);

        // Let the sidebar finish sliding in: while it animates it is narrower
        // than its full width and the reader is meant to show beside it.
        std::thread::sleep(std::time::Duration::from_millis(250));

        let mut terminal = Terminal::new(TestBackend::new(84, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render overlay");

        // Well below the outline's last entry, so anything here would be the
        // reader's own text showing through a background-only fill.
        let buffer = terminal.backend().buffer();
        let beneath =
            (0..app.sidebar_width().saturating_sub(1)).fold(String::new(), |mut text, x| {
                text.push_str(buffer[(x, 34)].symbol());
                text
            });
        assert!(
            beneath.trim().is_empty(),
            "the reader shows through the overlaid sidebar: {beneath:?}"
        );
    }

    #[test]
    fn the_quick_guide_shows_every_group_without_clipping() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        app.update(Message::Key {
            key: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render help");

        let buffer = terminal.backend().buffer();
        let rows = (0..40)
            .map(|y| {
                (0..120).fold(String::new(), |mut text, x| {
                    text.push_str(buffer[(x, y)].symbol());
                    text
                })
            })
            .collect::<Vec<_>>();
        let screen = rows.join("\n");

        for group in ["READ", "EDIT", "NAVIGATE", "SELECT", "FIND", "APPEARANCE"] {
            assert!(screen.contains(group), "the `{group}` group was clipped");
        }
        assert!(screen.contains("Press any key to return"));
    }

    #[test]
    fn renders_attached_overlay_and_fullscreen_dimensions() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        for (width, height) in [(120, 40), (84, 30), (60, 24)] {
            app.update(Message::Resize {
                width,
                height,
                at: Instant::now(),
            });
            let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
            terminal
                .draw(|frame| super::render(frame, &app))
                .expect("render responsive shell");
        }
    }

    #[test]
    fn renders_editor_syntax_and_unsaved_prompt() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        app.update(Message::Key {
            key: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('e'),
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });
        app.update(Message::Key {
            key: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::End,
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });
        app.update(Message::Key {
            key: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('!'),
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });
        app.update(Message::Key {
            key: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render editor prompt");
        let rendered =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut output, cell| {
                    output.push_str(cell.symbol());
                    output
                });

        assert!(rendered.contains("UNSAVED CHANGES"));
    }

    #[test]
    fn a_long_editor_line_wraps_and_only_its_first_row_is_numbered() {
        let path = std::env::temp_dir().join(format!(
            "markr-view-wrap-{}-{:?}.md",
            std::process::id(),
            Instant::now()
        ));
        // One line far too long for the reader, so it has to wrap.
        let long = ["alpha"; 60].join(" ");
        std::fs::write(&path, &long).expect("wrap fixture");

        let workspace = Workspace::open(Some(path.clone()), true).expect("workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        app.update(Message::Key {
            key: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('e'),
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });
        assert!(app.editor_wrap().len() > 1, "the fixture line should wrap");

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render wrapped editor");

        let buffer = terminal.backend().buffer();
        let rows: Vec<Vec<char>> = (0..app.editor_wrap().len() as u16)
            .map(|index| {
                (0..120)
                    .map(|x| {
                        buffer[(x, index + 2)]
                            .symbol()
                            .chars()
                            .next()
                            .unwrap_or(' ')
                    })
                    .collect()
            })
            .collect();

        // The sidebar shares the row, so find where the editor's own gutter
        // begins rather than assuming it starts at column zero.
        let gutter_width = app.editor_gutter_width();
        let start = rows[0]
            .windows(5)
            .position(|window| window == [' ', '1', ' ', '│', ' '])
            .expect("the first row carries its line number");

        let text_of = |row: &Vec<char>| -> String {
            row[start + gutter_width - 1..].iter().collect::<String>()
        };

        for (index, row) in rows.iter().enumerate().skip(1) {
            let gutter: String = row[start..start + gutter_width - 1].iter().collect();
            assert!(
                gutter.trim().is_empty(),
                "continuation row {index} should leave the gutter blank: {gutter:?}"
            );
            assert!(
                text_of(row).trim_start().starts_with("alpha"),
                "continuation row {index} should carry text"
            );
        }

        // Nothing is lost or duplicated: the rows together spell the line.
        let joined: String = rows
            .iter()
            .map(|row| text_of(row).trim().to_owned())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(joined.replace(' ', ""), long.replace(' ', ""));

        std::fs::remove_file(path).expect("remove wrap fixture");
    }

    #[test]
    fn renders_every_document_position_without_panicking() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");

        for scroll in 0..=app.document_layout.lines.len() {
            app.scroll = scroll;
            terminal
                .draw(|frame| super::render(frame, &app))
                .expect("render position");
        }
    }

    #[test]
    fn renders_clipped_images_with_the_iterm_protocol() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Iterm2);
        let mut app = App::new(workspace, picker, Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");

        for scroll in 0..=app.document_layout.lines.len() {
            app.scroll = scroll;
            terminal
                .draw(|frame| super::render(frame, &app))
                .expect("render clipped image");
        }
    }

    #[test]
    fn renders_every_document_position_with_the_kitty_protocol() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut app = App::new(workspace, picker, Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");

        for scroll in 0..=app.document_layout.lines.len() {
            app.scroll = scroll;
            terminal
                .draw(|frame| super::render(frame, &app))
                .expect("render sliced kitty image");
        }
    }

    #[test]
    fn repeatedly_crosses_the_kitty_image_boundary_without_panicking() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut app = App::new(workspace, picker, Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        let region = app
            .document_layout
            .image_regions
            .first()
            .expect("image region");
        let Asset::Ready { rows, .. } = app.images.asset(&region.src).expect("loaded image asset")
        else {
            panic!("loaded image");
        };
        let image_end = region.line + usize::from(*rows);
        let positions = [image_end.saturating_sub(1), image_end];
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");

        for _ in 0..250 {
            for scroll in positions {
                app.scroll = scroll;
                terminal
                    .draw(|frame| super::render(frame, &app))
                    .expect("render image boundary");
            }
        }
    }
}
