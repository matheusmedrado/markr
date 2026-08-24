use std::ops::Range;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block as TuiBlock, Borders, Clear, List, ListItem, ListState, Paragraph, Widget,
};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus, ResponsiveMode, SidebarPanel};
use crate::explorer::EntryKind;
use crate::images::Asset;
use crate::layout;
use crate::selection;
use crate::syntax;
use crate::theme::Theme;

pub struct FloatingReader {
    theme: Theme,
    focused: bool,
}

impl FloatingReader {
    pub fn new(theme: Theme, focused: bool) -> Self {
        Self { theme, focused }
    }

    pub fn inner(area: Rect) -> Rect {
        Rect::new(
            area.x.saturating_add(3),
            area.y.saturating_add(1),
            area.width.saturating_sub(5),
            area.height.saturating_sub(2),
        )
    }
}

impl Widget for FloatingReader {
    fn render(self, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
        paint_rounded_surface(
            buffer,
            area,
            self.theme.reader_background,
            self.theme.background,
        );
        render_rounded_border(
            buffer,
            area,
            self.theme.reader_border,
            self.theme.background,
        );

        let marker_start = area.y.saturating_add(2);
        let marker_end = area.y.saturating_add(5).min(area.bottom());
        let marker_color = if self.focused {
            self.theme.accent
        } else {
            self.theme.reader_border
        };
        for y in marker_start..marker_end {
            let cell = &mut buffer[(area.x, y)];
            cell.set_symbol("▎");
            cell.set_fg(marker_color);
            cell.set_bg(self.theme.reader_background);
        }
    }
}

struct RoundedPanel {
    theme: Theme,
}

impl RoundedPanel {
    fn new(theme: Theme) -> Self {
        Self { theme }
    }

    fn inner(area: Rect) -> Rect {
        let border_inner = rounded_border_inner(area);
        Rect::new(
            border_inner.x.saturating_add(2),
            border_inner.y,
            border_inner.width.saturating_sub(4),
            border_inner.height,
        )
    }
}

impl Widget for RoundedPanel {
    fn render(self, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
        paint_rounded_surface(
            buffer,
            area,
            self.theme.reader_background,
            self.theme.background,
        );
        render_rounded_border(
            buffer,
            area,
            self.theme.reader_border,
            self.theme.background,
        );
    }
}

struct RoundedSidebar {
    theme: Theme,
    focused: bool,
}

impl Widget for RoundedSidebar {
    fn render(self, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
        paint_rounded_surface(buffer, area, self.theme.surface, self.theme.background);
        render_rounded_border(buffer, area, self.theme.border, self.theme.background);

        let marker_start = area.y.saturating_add(2);
        let marker_end = area.y.saturating_add(5).min(area.bottom());
        let marker_color = if self.focused {
            self.theme.accent
        } else {
            self.theme.border
        };
        for y in marker_start..marker_end {
            let cell = &mut buffer[(area.x, y)];
            cell.set_symbol("▎");
            cell.set_fg(marker_color);
            cell.set_bg(self.theme.surface);
        }
    }
}

fn rounded_border_inner(area: Rect) -> Rect {
    TuiBlock::default().borders(Borders::ALL).inner(area)
}

fn paint_rounded_surface(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    background: Color,
    outer_background: Color,
) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buffer[(x, y)].set_bg(outer_background);
        }
    }

    if area.width < 2 || area.height < 2 {
        return;
    }

    let last_x = area.right().saturating_sub(1);
    let last_y = area.bottom().saturating_sub(1);
    for x in area.x.saturating_add(1)..last_x {
        buffer[(x, area.y)].set_bg(background);
        buffer[(x, last_y)].set_bg(background);
    }
    for y in area.y.saturating_add(1)..last_y {
        for x in area.x..area.right() {
            buffer[(x, y)].set_bg(background);
        }
    }
}

fn render_rounded_border(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    border: Color,
    outer_background: Color,
) {
    TuiBlock::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::default().fg(border))
        .render(area, buffer);

    if area.width >= 2 && area.height >= 2 {
        for (x, y) in [
            (area.x, area.y),
            (area.right().saturating_sub(1), area.y),
            (area.x, area.bottom().saturating_sub(1)),
            (
                area.right().saturating_sub(1),
                area.bottom().saturating_sub(1),
            ),
        ] {
            buffer[(x, y)].set_bg(outer_background);
        }
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    frame.render_widget(
        TuiBlock::default().style(Style::default().bg(app.theme.background)),
        frame.area(),
    );

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_header(frame, app, vertical[0]);
    render_body(frame, app, vertical[1]);
    render_status(frame, app, vertical[2]);

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
    let title = Line::from(vec![
        Span::styled("▰ MARKR", theme.accent()),
        Span::styled("  /  ", theme.muted()),
        if app.is_editing() {
            Span::styled("✎ EDIT  /  ", theme.accent())
        } else {
            Span::raw("")
        },
        Span::styled(app.workspace.display_name(), theme.title()),
    ]);
    frame.render_widget(Paragraph::new(title), area);
}

fn render_body(frame: &mut Frame, app: &App, area: Rect) {
    let now = Instant::now();
    if app.responsive_mode() == ResponsiveMode::Fullscreen && app.sidebar_visible {
        clear_area(frame, area, app.theme.background);
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
            clear_area(frame, sidebar_area, app.theme.background);
        }
        render_sidebar(frame, app, sidebar_area);
    }
}

fn reader_area(app: &App, area: Rect) -> Rect {
    if app.responsive_mode() == ResponsiveMode::Fullscreen && app.sidebar_visible {
        return Rect::default();
    }
    let x = if app.responsive_mode() == ResponsiveMode::Attached && app.sidebar_visible {
        area.x.saturating_add(app.sidebar_width().saturating_add(1))
    } else {
        area.x
    };
    let width = area.width.saturating_sub(x.saturating_sub(area.x));
    let horizontal_gutter = if area.width < 72 { 0 } else { 1 };
    Rect::new(
        x.saturating_add(horizontal_gutter),
        area.y,
        width.saturating_sub(horizontal_gutter.saturating_mul(2)),
        area.height,
    )
}

fn clear_area(frame: &mut Frame, area: Rect, background: Color) {
    frame.render_widget(
        TuiBlock::default().style(Style::default().bg(background)),
        area,
    );
}

fn render_reader(frame: &mut Frame, app: &App, document_area: Rect) {
    let theme = app.theme;
    frame.render_widget(
        FloatingReader::new(theme, app.focus == Focus::Document),
        document_area,
    );
    let inner = FloatingReader::inner(document_area);
    if app.is_editing() {
        render_editor(frame, app, inner);
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
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::default().fg(theme.text).bg(theme.reader_background));
    frame.render_widget(paragraph, inner);

    let content_width = usize::from(inner.width).min(layout::MAX_CONTENT_WIDTH);
    let image_regions: Vec<(String, usize)> = app
        .document_layout
        .image_regions
        .iter()
        .map(|region| (region.src.clone(), region.line))
        .collect();
    for (src, line) in image_regions {
        let Some(Asset::Ready { cols, rows, .. }) = app.images.asset(&src) else {
            continue;
        };
        if let Some(position) = image_position(inner, content_width, line, app.scroll, *cols, *rows)
        {
            let Some(Asset::Ready { protocol, .. }) = app.images.asset(&src) else {
                continue;
            };
            frame.render_widget(SlicedImage::new(protocol, position), inner);
        }
    }
}

fn render_editor(frame: &mut Frame, app: &App, inner: Rect) {
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = app.theme;
    let lines = app.editor_lines();
    let cursor = app.editor_cursor();
    let highlighted_lines = app
        .editor_text()
        .map(|text| syntax::highlight(Some("markdown"), &text, theme))
        .unwrap_or_default();
    let line_number_width = lines.len().max(1).to_string().len();
    let prefix_width = line_number_width.saturating_add(3);
    let visible_width = app.editor_content_width();
    let horizontal_scroll = app.editor_horizontal_scroll;
    let start = app.editor_scroll.min(lines.len());
    let visible_lines = lines.iter().skip(start).take(usize::from(inner.height));
    let text = visible_lines
        .enumerate()
        .map(|(line_index, line)| {
            let absolute_line = start + line_index;
            let is_cursor_line = cursor.is_some_and(|cursor| cursor.line == absolute_line);
            let line_number_style = if is_cursor_line {
                theme.accent()
            } else {
                theme.muted()
            };
            let prefix = format!(
                "{:>line_number_width$} │ ",
                absolute_line + 1,
                line_number_width = line_number_width
            );
            let content = highlighted_lines
                .get(absolute_line)
                .map(|line| {
                    slice_spans_by_columns(
                        line,
                        horizontal_scroll,
                        horizontal_scroll.saturating_add(visible_width),
                        theme.reader_background,
                    )
                })
                .unwrap_or_else(|| {
                    vec![Span::styled(
                        selection::slice_by_columns(
                            line,
                            horizontal_scroll,
                            horizontal_scroll.saturating_add(visible_width),
                        ),
                        Style::default().fg(theme.text),
                    )]
                });
            let mut spans = vec![Span::styled(prefix, line_number_style)];
            spans.extend(content);
            Line::from(spans)
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(Text::from(text))
            .style(Style::default().fg(theme.text).bg(theme.reader_background)),
        inner,
    );

    let Some(cursor) = cursor else {
        return;
    };
    if cursor.line < start
        || cursor.line >= lines.len()
        || cursor.line >= start.saturating_add(usize::from(inner.height))
    {
        return;
    }
    let cursor_column = app
        .editor_cursor_display_column()
        .unwrap_or_default()
        .saturating_sub(horizontal_scroll)
        .min(visible_width);
    let x = inner
        .x
        .saturating_add(prefix_width as u16)
        .saturating_add(cursor_column as u16)
        .min(inner.right().saturating_sub(1));
    let y = inner
        .y
        .saturating_add(cursor.line.saturating_sub(start) as u16);
    frame.set_cursor_position(Position::new(x, y));
}

fn slice_spans_by_columns(
    spans: &[Span<'static>],
    start: usize,
    end: usize,
    background: Color,
) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut column: usize = 0;

    for span in spans {
        let mut segment = String::new();
        let style = span.style.bg(background);
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
    frame.render_widget(
        RoundedSidebar {
            theme: app.theme,
            focused: app.focus == Focus::Sidebar,
        },
        area,
    );
    let inner = rounded_border_inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let tabs_area = Rect::new(inner.x, inner.y, inner.width, 2);
    render_sidebar_tabs(frame, app, tabs_area);

    let list_area = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(2),
    );
    match app.sidebar_panel {
        SidebarPanel::Outline => render_outline(frame, app, list_area),
        SidebarPanel::Files => render_files(frame, app, list_area),
    }
}

fn render_sidebar_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let outline_label = "  HEADINGS  ";
    let files_label = "  FILES  ";

    let tabs = Line::from(vec![
        Span::styled(outline_label, sidebar_tab_style(app, SidebarPanel::Outline)),
        Span::raw(" "),
        Span::styled(files_label, sidebar_tab_style(app, SidebarPanel::Files)),
    ]);
    frame.render_widget(Paragraph::new(tabs), area);

    let (active_label, offset) = if app.sidebar_panel == SidebarPanel::Outline {
        (outline_label, 0)
    } else {
        (files_label, outline_label.len() + 1)
    };
    let underline = "─".repeat(active_label.len());
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            underline,
            Style::default().fg(if app.focus == Focus::Sidebar {
                app.theme.accent
            } else {
                app.theme.border
            }),
        )))
        .alignment(Alignment::Left),
        Rect::new(
            area.x + offset as u16,
            area.y + 1,
            area.width.saturating_sub(offset as u16),
            1,
        ),
    );
}

fn render_outline(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let padding = "  ";
    let items: Vec<ListItem> = if app.document.outline.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No headings",
            theme.muted(),
        )))]
    } else {
        app.document
            .outline
            .iter()
            .enumerate()
            .map(|(index, heading)| {
                let indent = "  ".repeat(heading.level.saturating_sub(1) as usize);
                let marker = if index == app.outline_selected {
                    "▌ "
                } else {
                    "· "
                };
                ListItem::new(Line::from(vec![
                    Span::styled(padding, Style::default()),
                    Span::styled(marker, Style::default().fg(theme.accent)),
                    Span::styled(
                        format!("{indent}{}", heading.title),
                        Style::default().fg(theme.chrome_muted),
                    ),
                ]))
            })
            .collect()
    };

    let highlight_style = if app.focus == Focus::Sidebar {
        Style::default()
            .fg(theme.chrome_text)
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let list = List::new(items)
        .style(Style::default().bg(theme.surface))
        .highlight_style(highlight_style);
    let mut state = ListState::default()
        .with_selected((!app.document.outline.is_empty()).then_some(app.outline_selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_files(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let padding = "  ";
    let items: Vec<ListItem> = if app.file_explorer.entries().is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  Empty directory",
            theme.muted(),
        )))]
    } else {
        app.file_explorer
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let active = app.workspace.active_path() == Some(entry.path.as_path());
                let selected = index == app.file_explorer.selected();
                let marker = if selected {
                    "▌ "
                } else {
                    match entry.kind {
                        EntryKind::Parent => "↰ ",
                        EntryKind::Directory => "▸ ",
                        EntryKind::Markdown if active => "● ",
                        EntryKind::Markdown => "◇ ",
                        EntryKind::File => "· ",
                    }
                };
                let suffix = matches!(entry.kind, EntryKind::Directory).then_some("/");
                let style = if active {
                    Style::default().fg(theme.accent)
                } else if matches!(entry.kind, EntryKind::File) {
                    Style::default().fg(theme.border)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(padding, Style::default()),
                    Span::styled(marker, style.fg(theme.accent)),
                    Span::styled(
                        format!("{}{}", entry.name, suffix.unwrap_or_default()),
                        style,
                    ),
                ]))
            })
            .collect()
    };

    let highlight_style = if app.focus == Focus::Sidebar {
        Style::default()
            .fg(theme.chrome_text)
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let list = List::new(items)
        .style(Style::default().bg(theme.surface))
        .highlight_style(highlight_style);
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

fn sidebar_tab_style(app: &App, panel: SidebarPanel) -> Style {
    if app.sidebar_panel == panel {
        if app.focus == Focus::Sidebar {
            Style::default()
                .fg(app.theme.accent)
                .bg(app.theme.accent_soft)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.text_muted)
        }
    } else {
        app.theme.muted()
    }
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let left = if let Some(input) = &app.search_input {
        Line::from(vec![
            Span::styled("/", theme.accent()),
            Span::styled(input, theme.text),
            Span::styled("  Enter search · Esc cancel", theme.muted()),
        ])
    } else if let Some(message) = app.temporary_message(Instant::now()) {
        Line::from(Span::styled(message, theme.accent()))
    } else if app.is_editing() {
        let mut spans = Vec::new();
        if app.external_change_detected {
            spans.push(Span::styled("⚠ file changed on disk  ", theme.warning));
        }
        spans.extend([
            Span::styled("EDIT", theme.accent()),
            Span::styled(
                if app.editor_dirty() {
                    "  unsaved changes  "
                } else {
                    "  saved  "
                },
                theme.muted(),
            ),
            Span::styled("Ctrl-S", theme.accent()),
            Span::styled(" save  ", theme.muted()),
            Span::styled("Ctrl-Z/Y", theme.accent()),
            Span::styled(" undo/redo  ", theme.muted()),
            Span::styled("Esc", theme.accent()),
            Span::styled(" leave", theme.muted()),
        ]);
        Line::from(spans)
    } else if let Some((current, total)) = app.search_result_position() {
        Line::from(vec![
            Span::styled(format!("/{}", app.search_query), theme.accent()),
            Span::styled(format!("  {current}/{total}  "), theme.muted()),
            Span::styled("n", theme.accent()),
            Span::styled(" next  ", theme.muted()),
            Span::styled("N", theme.accent()),
            Span::styled(" previous", theme.muted()),
        ])
    } else if !app.search_query.is_empty() {
        Line::from(vec![
            Span::styled(format!("/{}", app.search_query), theme.accent()),
            Span::styled("  no matches  ", theme.muted()),
            Span::styled("/", theme.accent()),
            Span::styled(" edit", theme.muted()),
        ])
    } else if app.focus == Focus::Sidebar && app.sidebar_panel == SidebarPanel::Files {
        Line::from(vec![
            Span::styled("h/⌫", theme.accent()),
            Span::styled(" parent  ", theme.muted()),
            Span::styled("l/Enter", theme.accent()),
            Span::styled(" open  ", theme.muted()),
            Span::styled("r", theme.accent()),
            Span::styled(" refresh  ", theme.muted()),
            Span::styled("TAB", theme.accent()),
            Span::styled(" focus  ", theme.muted()),
            Span::styled("e", theme.accent()),
            Span::styled(" edit", theme.muted()),
        ])
    } else {
        Line::from(vec![
            Span::styled("/", theme.accent()),
            Span::styled(" search  ", theme.muted()),
            Span::styled("TAB", theme.accent()),
            Span::styled(" focus  ", theme.muted()),
            Span::styled("e", theme.accent()),
            Span::styled(" edit  ", theme.muted()),
            Span::styled("?", theme.accent()),
            Span::styled(" help", theme.muted()),
        ])
    };
    let file_number = if app.workspace.files.is_empty() {
        "stdin".to_string()
    } else {
        format!(
            "{}/{}",
            app.workspace.selected + 1,
            app.workspace.files.len()
        )
    };
    let right = if let Some(cursor) = app.editor_cursor() {
        format!(
            "{} · line {}, column {}",
            file_number,
            cursor.line + 1,
            cursor.column + 1
        )
    } else if let Some((current, total)) = app.search_result_position() {
        format!("search {current}/{total} · {file_number}")
    } else {
        format!("{} · line {}", file_number, app.scroll.saturating_add(1))
    };
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Min(1)])
        .split(area);
    frame.render_widget(Paragraph::new(left), chunks[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(right, theme.muted())).alignment(Alignment::Right),
        chunks[1],
    );
}

fn render_help(frame: &mut Frame, app: &App) {
    let progress = app.help_progress(Instant::now());
    let width = 40 + (20.0 * progress) as u16;
    let height = 40 + (20.0 * progress) as u16;
    let area = centered_rect(width, height, frame.area());
    let theme = app.theme;
    let text = Text::from(vec![
        Line::from(Span::styled(" MARKR / QUICK GUIDE ", theme.accent())),
        Line::default(),
        Line::from(" ↑↓ / j k     navigate document or sidebar"),
        Line::from(" Tab           switch sidebar/document focus"),
        Line::from(" Enter         open or activate selection"),
        Line::from(" ← / →         switch Outline / Files"),
        Line::from(" Backspace     parent directory"),
        Line::from(" [ / ]         previous / next document"),
        Line::from(" g / G         top / bottom"),
        Line::from(" Ctrl-u/d       page up / down"),
        Line::from(" t              toggle outline"),
        Line::from(" T              cycle color theme"),
        Line::from(" 1 / 2          outline / files panel"),
        Line::from(" Enter / l      open file or directory"),
        Line::from(" h / Backspace  explorer parent directory"),
        Line::from(" r              refresh explorer"),
        Line::from(" e              edit the active file"),
        Line::from(" mouse click    place editor cursor"),
        Line::from(" Ctrl-S         save edits"),
        Line::from(" Ctrl-Z/Y       undo / redo"),
        Line::from(" Esc            leave editor"),
        Line::from(" s/d            save / discard prompt"),
        Line::from(" o/r            overwrite / reload changed file"),
        Line::from(" /              search rendered text"),
        Line::from(" n / N          next / previous match"),
        Line::from(" v              start keyboard selection"),
        Line::from(" arrows / hjkl  extend selection"),
        Line::from(" y              copy selection"),
        Line::from(" mouse drag     select inside reader"),
        Line::from(" q              quit"),
        Line::default(),
        Line::from(Span::styled(" Press any key to return ", theme.muted())),
    ]);
    frame.render_widget(Clear, area);
    frame.render_widget(RoundedPanel::new(theme), area);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(theme.text).bg(theme.reader_background)),
        RoundedPanel::inner(area),
    );
}

fn render_unsaved_prompt(frame: &mut Frame, app: &App) {
    let area = centered_rect(58, 30, frame.area());
    let theme = app.theme;
    let text = Text::from(vec![
        Line::from(Span::styled(" MARKR / UNSAVED CHANGES ", theme.accent())),
        Line::default(),
        Line::from("Leave the editor with unsaved changes?"),
        Line::default(),
        Line::from(vec![
            Span::styled("s / Enter", theme.accent()),
            Span::styled(" save  ", theme.muted()),
            Span::styled("d", theme.accent()),
            Span::styled(" discard  ", theme.muted()),
            Span::styled("Esc", theme.accent()),
            Span::styled(" continue editing", theme.muted()),
        ]),
    ]);
    frame.render_widget(Clear, area);
    frame.render_widget(RoundedPanel::new(theme), area);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(theme.text).bg(theme.reader_background)),
        RoundedPanel::inner(area),
    );
}

fn render_external_prompt(frame: &mut Frame, app: &App) {
    let area = centered_rect(62, 30, frame.area());
    let theme = app.theme;
    let text = Text::from(vec![
        Line::from(Span::styled(" MARKR / FILE CHANGED ", theme.warning)),
        Line::default(),
        Line::from("This file changed outside MarkR while editing."),
        Line::from("Choose how to continue:"),
        Line::default(),
        Line::from(vec![
            Span::styled("o / Enter", theme.accent()),
            Span::styled(" overwrite  ", theme.muted()),
            Span::styled("r", theme.accent()),
            Span::styled(" reload  ", theme.muted()),
            Span::styled("Esc", theme.accent()),
            Span::styled(" back", theme.muted()),
        ]),
    ]);
    frame.render_widget(Clear, area);
    frame.render_widget(RoundedPanel::new(theme), area);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(theme.text).bg(theme.reader_background)),
        RoundedPanel::inner(area),
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn image_position(
    inner: Rect,
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

    let content_width = content_width.min(usize::from(inner.width)) as u16;
    let width = image_width.min(content_width);
    let content_x = (inner.width.saturating_sub(content_width)) / 2;
    let x = content_x + (content_width.saturating_sub(width)) / 2;
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

    use super::{FloatingReader, highlight_search_line, highlight_selection_line, image_position};
    use crate::app::{App, Message};
    use crate::images::Asset;
    use crate::selection::CursorPosition;
    use crate::theme::Theme;
    use crate::workspace::Workspace;

    #[test]
    fn centers_images_inside_the_content_column() {
        let position = image_position(Rect::new(10, 5, 100, 20), 88, 4, 0, 40, 10).unwrap();

        assert_eq!(position.x, 30);
        assert_eq!(position.y, 4);
    }

    #[test]
    fn keeps_partially_visible_images_on_screen_after_scroll() {
        let position = image_position(Rect::new(10, 5, 100, 20), 88, 4, 8, 40, 10).unwrap();

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
    fn floating_reader_is_a_rounded_panel_with_an_editorial_marker() {
        let theme = Theme::default();
        let mut terminal = Terminal::new(TestBackend::new(20, 10)).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(FloatingReader::new(theme, true), Rect::new(2, 2, 12, 6));
            })
            .expect("render reader");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(2, 2)].symbol(), "╭");
        assert_eq!(buffer[(13, 2)].symbol(), "╮");
        assert_eq!(buffer[(2, 7)].symbol(), "╰");
        assert_eq!(buffer[(13, 7)].symbol(), "╯");
        assert_eq!(buffer[(2, 2)].bg, theme.background);
        assert_eq!(buffer[(13, 7)].bg, theme.background);
        assert_eq!(buffer[(5, 4)].bg, theme.reader_background);
        assert_eq!(buffer[(2, 4)].symbol(), "▎");
        assert_eq!(buffer[(2, 4)].fg, theme.accent);
        assert_eq!(buffer[(2, 4)].bg, theme.reader_background);
        assert_eq!(buffer[(14, 3)].symbol(), " ");
        assert_eq!(buffer[(14, 3)].bg, Color::Reset);
        assert_eq!(buffer[(3, 8)].symbol(), " ");
        assert_eq!(buffer[(3, 8)].bg, Color::Reset);
    }

    #[test]
    fn shell_uses_the_thematic_background_around_panels() {
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
        assert_eq!(buffer[(0, 0)].bg, theme.background);
        assert_eq!(buffer[(31, 2)].bg, theme.background);
        assert_eq!(buffer[(0, 1)].symbol(), "╭");
        assert_eq!(buffer[(29, 1)].symbol(), "╮");
        assert_eq!(buffer[(0, 38)].symbol(), "╰");
        assert_eq!(buffer[(29, 38)].symbol(), "╯");
        assert_eq!(buffer[(0, 1)].bg, theme.background);
        assert_eq!(buffer[(0, 3)].symbol(), "▎");
        assert_eq!(buffer[(0, 3)].fg, theme.border);
        assert_eq!(buffer[(32, 1)].bg, theme.background);
        assert_eq!(buffer[(35, 2)].bg, Theme::default().reader_background);
        assert_eq!(buffer[(119, 1)].bg, theme.background);
        assert_eq!(buffer[(119, 1)].symbol(), " ");
        assert_eq!(buffer[(32, 3)].symbol(), "▎");
        assert_eq!(buffer[(32, 3)].fg, Theme::default().accent);
    }

    #[test]
    fn sidebar_uses_a_thematic_surface_and_renders_tab_underline() {
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
        assert_eq!(buffer[(1, 5)].bg, theme.surface);
        assert_eq!(buffer[(0, 1)].symbol(), "╭");
        assert_eq!(buffer[(0, 1)].bg, theme.background);
        assert_eq!(buffer[(2, 3)].fg, theme.border);
    }

    #[test]
    fn focused_sidebar_uses_the_same_editorial_marker_as_the_reader() {
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
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ),
            at: Instant::now(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &app))
            .expect("render focused sidebar");

        let buffer = terminal.backend().buffer();
        let theme = Theme::default();
        assert_eq!(buffer[(0, 3)].symbol(), "▎");
        assert_eq!(buffer[(0, 3)].fg, theme.accent);
        assert_eq!(buffer[(2, 2)].bg, theme.accent_soft);
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
