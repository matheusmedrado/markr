use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block as TuiBlock, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui_image::sliced::{SignedPosition, SlicedImage};

use crate::app::{App, Focus, SidebarPanel};
use crate::explorer::EntryKind;
use crate::images::Asset;
use crate::layout;

pub fn render(frame: &mut Frame, app: &App) {
    let theme = app.theme;
    frame.render_widget(
        TuiBlock::default().style(Style::default().bg(theme.background)),
        frame.area(),
    );

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_header(frame, app, vertical[0]);
    render_body(frame, app, vertical[1]);
    render_status(frame, app, vertical[2]);

    if app.help_visible {
        render_help(frame, app);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let file_number = if app.workspace.files.is_empty() {
        "stdin".to_string()
    } else {
        format!(
            "{}/{}",
            app.workspace.selected + 1,
            app.workspace.files.len()
        )
    };
    let title = Line::from(vec![
        Span::styled("  MARKR", theme.accent()),
        Span::styled("  /  ", theme.muted()),
        Span::styled(app.workspace.display_name(), theme.title()),
        Span::styled(format!("  {file_number}"), theme.muted()),
    ]);
    frame.render_widget(Paragraph::new(title), area);
}

fn render_body(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let chunks = if app.sidebar_visible {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(app.sidebar_width()), Constraint::Min(1)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1)])
            .split(area)
    };

    if app.sidebar_visible {
        render_sidebar(frame, app, chunks[0]);
    }

    let document_area = chunks[chunks.len() - 1];
    let document_block = TuiBlock::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(theme.border))
        .padding(ratatui::widgets::Padding::horizontal(2));
    let inner = document_block.inner(document_area);
    let start = app.scroll.min(app.document_layout.lines.len());
    let end = start.saturating_add(usize::from(inner.height));
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
            highlight_search_line(line, &highlights, theme)
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::default().fg(theme.text).bg(theme.background))
        .block(document_block);
    frame.render_widget(paragraph, document_area);

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

fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    match app.sidebar_panel {
        SidebarPanel::Outline => render_outline(frame, app, area),
        SidebarPanel::Files => render_files(frame, app, area),
    }
}

fn render_outline(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
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
                    "◆ "
                } else {
                    "· "
                };
                let style = if index == app.outline_selected && app.focus == Focus::Sidebar {
                    Style::default()
                        .fg(theme.text)
                        .bg(theme.surface_active)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                ListItem::new(Line::from(Span::styled(
                    format!("{marker}{indent}{}", heading.title),
                    style,
                )))
            })
            .collect()
    };

    let border_style = if app.focus == Focus::Sidebar && app.sidebar_panel == SidebarPanel::Outline
    {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };
    let list = List::new(items)
        .style(Style::default().bg(theme.surface))
        .block(
            TuiBlock::default()
                .title(Line::from(vec![
                    Span::styled(" OUTLINE ", sidebar_tab_style(app, SidebarPanel::Outline)),
                    Span::styled(" FILES ", sidebar_tab_style(app, SidebarPanel::Files)),
                ]))
                .borders(Borders::ALL)
                .border_style(border_style),
        );
    frame.render_widget(list, area);
}

fn render_files(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let items: Vec<ListItem> = if app.file_explorer.entries().is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  Empty directory",
            theme.muted(),
        )))]
    } else {
        app.file_explorer
            .entries()
            .iter()
            .map(|entry| {
                let active = app.workspace.active_path() == Some(entry.path.as_path());
                let marker = match entry.kind {
                    EntryKind::Parent => "↰ ",
                    EntryKind::Directory => "▸ ",
                    EntryKind::Markdown if active => "● ",
                    EntryKind::Markdown => "◇ ",
                    EntryKind::File => "· ",
                };
                let suffix = matches!(entry.kind, EntryKind::Directory).then_some("/");
                let style = if active {
                    Style::default().fg(theme.accent)
                } else if matches!(entry.kind, EntryKind::File) {
                    Style::default().fg(theme.border)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                ListItem::new(Line::from(Span::styled(
                    format!("{marker}{}{}", entry.name, suffix.unwrap_or_default()),
                    style,
                )))
            })
            .collect()
    };

    let border_style = if app.focus == Focus::Sidebar && app.sidebar_panel == SidebarPanel::Files {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };
    let block = TuiBlock::default()
        .title(Line::from(vec![
            Span::styled(" OUTLINE ", sidebar_tab_style(app, SidebarPanel::Outline)),
            Span::styled(" FILES ", sidebar_tab_style(app, SidebarPanel::Files)),
        ]))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(theme.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);
    let directory = Text::from(vec![
        Line::from(Span::styled(
            format!(" {}", app.file_explorer.directory().display()),
            theme.title(),
        )),
        Line::from(Span::styled(
            " h parent · l open · r refresh",
            theme.muted(),
        )),
    ]);
    frame.render_widget(Paragraph::new(directory), chunks[0]);

    let highlight_style = if app.focus == Focus::Sidebar {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface_active)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .style(Style::default().bg(theme.surface))
        .highlight_style(highlight_style);
    let mut state = ListState::default().with_selected(
        (!app.file_explorer.entries().is_empty()).then_some(app.file_explorer.selected()),
    );
    frame.render_stateful_widget(list, chunks[1], &mut state);
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

fn sidebar_tab_style(app: &App, panel: SidebarPanel) -> Style {
    if app.sidebar_panel == panel {
        app.theme.accent()
    } else {
        app.theme.muted()
    }
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let left = if let Some(input) = &app.search_input {
        Line::from(vec![
            Span::styled("  /", theme.accent()),
            Span::styled(input, theme.text),
            Span::styled("  Enter search  Esc cancel", theme.muted()),
        ])
    } else if let Some(error) = &app.error {
        Line::from(Span::styled(format!("  {error}"), theme.accent()))
    } else if let Some((current, total)) = app.search_result_position() {
        Line::from(vec![
            Span::styled(format!("  /{}", app.search_query), theme.accent()),
            Span::styled(format!("  {current}/{total}  "), theme.muted()),
            Span::styled("n", theme.accent()),
            Span::styled(" next  ", theme.muted()),
            Span::styled("N", theme.accent()),
            Span::styled(" previous", theme.muted()),
        ])
    } else if !app.search_query.is_empty() {
        Line::from(vec![
            Span::styled(format!("  /{}", app.search_query), theme.accent()),
            Span::styled("  no matches  ", theme.muted()),
            Span::styled("/", theme.accent()),
            Span::styled(" edit", theme.muted()),
        ])
    } else if app.focus == Focus::Sidebar && app.sidebar_panel == SidebarPanel::Files {
        Line::from(vec![
            Span::styled("  h/←", theme.accent()),
            Span::styled(" parent  ", theme.muted()),
            Span::styled("l/→/Enter", theme.accent()),
            Span::styled(" open  ", theme.muted()),
            Span::styled("r", theme.accent()),
            Span::styled(" refresh", theme.muted()),
        ])
    } else {
        Line::from(vec![
            Span::styled("  ", theme.muted()),
            Span::styled("TAB", theme.accent()),
            Span::styled(" focus  ", theme.muted()),
            Span::styled("?", theme.accent()),
            Span::styled(" help  ", theme.muted()),
            Span::styled("q", theme.accent()),
            Span::styled(" quit", theme.muted()),
        ])
    };
    let right = Span::styled(format!("MarkR · {} theme  ", theme.name), theme.muted());
    let mut spans = left.spans;
    spans.push(Span::raw(" "));
    spans.push(right);
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Right), area);
}

fn render_help(frame: &mut Frame, app: &App) {
    let area = centered_rect(60, 60, frame.area());
    let theme = app.theme;
    let text = Text::from(vec![
        Line::from(Span::styled(" MARKR / QUICK GUIDE ", theme.accent())),
        Line::default(),
        Line::from(" ↑↓ / j k     navigate document or sidebar"),
        Line::from(" Tab           switch sidebar/document focus"),
        Line::from(" Enter         open outline section"),
        Line::from(" [ / ]         previous / next document"),
        Line::from(" g / G         top / bottom"),
        Line::from(" Ctrl-u/d       page up / down"),
        Line::from(" t              toggle outline"),
        Line::from(" T              cycle color theme"),
        Line::from(" 1 / 2          outline / files panel"),
        Line::from(" Enter / l / →  open file or directory"),
        Line::from(" h / ←          explorer parent directory"),
        Line::from(" r              refresh explorer"),
        Line::from(" /              search rendered text"),
        Line::from(" n / N          next / previous match"),
        Line::from(" q              quit"),
        Line::default(),
        Line::from(Span::styled(" Press any key to return ", theme.muted())),
    ]);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(theme.text).bg(theme.surface))
            .block(
                TuiBlock::default()
                    .borders(Borders::ALL)
                    .border_style(theme.accent()),
            ),
        area,
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

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui_image::picker::{Picker, ProtocolType};

    use super::{highlight_search_line, image_position};
    use crate::app::{App, Message};
    use crate::images::Asset;
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
    fn renders_every_document_position_without_panicking() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 40,
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
