use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block as TuiBlock, Borders, Clear, List, ListItem, Paragraph};

use crate::app::{App, Focus};
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
            .constraints([Constraint::Length(29), Constraint::Min(1)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1)])
            .split(area)
    };

    if app.sidebar_visible {
        render_outline(frame, app, chunks[0]);
    }

    let document_area = chunks[chunks.len() - 1];
    let document_block = TuiBlock::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(theme.border))
        .padding(ratatui::widgets::Padding::horizontal(2));
    let document_layout = layout::build(
        &app.document,
        document_block.inner(document_area).width,
        theme,
    );
    let paragraph = Paragraph::new(Text::from(document_layout.lines))
        .style(Style::default().fg(theme.text).bg(theme.background))
        .block(document_block)
        .scroll((app.scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(paragraph, document_area);
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
                let style = if index == app.outline_selected && app.focus == Focus::Outline {
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

    let border_style = if app.focus == Focus::Outline {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };
    let list = List::new(items)
        .style(Style::default().bg(theme.surface))
        .block(
            TuiBlock::default()
                .title(Span::styled(" OUTLINE ", theme.muted()))
                .borders(Borders::ALL)
                .border_style(border_style),
        );
    frame.render_widget(list, area);
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let left = if let Some(error) = &app.error {
        Line::from(Span::styled(format!("  {error}"), theme.accent()))
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
    let right = Span::styled("MarkR · calm tools for living documents  ", theme.muted());
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
        Line::from(" ↑↓ / j k     navigate document or outline"),
        Line::from(" Tab           switch focus"),
        Line::from(" Enter         open outline section"),
        Line::from(" [ / ]         previous / next document"),
        Line::from(" g / G         top / bottom"),
        Line::from(" Ctrl-u/d       page up / down"),
        Line::from(" t              toggle outline"),
        Line::from(" q / Esc        quit"),
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
