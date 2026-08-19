use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::layout;
use crate::markdown::Document;
use crate::theme::Theme;
use crate::workspace::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Outline,
    Document,
}

#[derive(Debug)]
pub enum Message {
    Key(KeyEvent),
    Tick,
    Resize { width: u16, height: u16 },
}

#[derive(Debug)]
pub struct App {
    pub workspace: Workspace,
    pub document: Document,
    pub theme: Theme,
    pub focus: Focus,
    pub scroll: usize,
    pub outline_selected: usize,
    pub sidebar_visible: bool,
    pub help_visible: bool,
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub error: Option<String>,
    pub search_query: String,
    pub search_input: Option<String>,
    search_matches: Vec<usize>,
    search_selected: usize,
    last_modified: Option<SystemTime>,
    quit: bool,
}

impl App {
    pub fn new(workspace: Workspace) -> Result<Self, Box<dyn std::error::Error>> {
        let document = Document::parse(&workspace.reload_content()?);
        let last_modified = modified_time(workspace.active_path());
        Ok(Self {
            workspace,
            document,
            theme: Theme::default(),
            focus: Focus::Document,
            scroll: 0,
            outline_selected: 0,
            sidebar_visible: true,
            help_visible: false,
            terminal_width: 120,
            terminal_height: 40,
            error: None,
            search_query: String::new(),
            search_input: None,
            search_matches: Vec::new(),
            search_selected: 0,
            last_modified,
            quit: false,
        })
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn document_width(&self) -> u16 {
        let sidebar_width = if self.sidebar_visible { 29 } else { 0 };
        self.terminal_width
            .saturating_sub(sidebar_width)
            .saturating_sub(5)
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Key(key) => self.handle_key(key),
            Message::Tick => self.reload_if_changed(),
            Message::Resize { width, height } => {
                self.terminal_width = width;
                self.terminal_height = height;
                self.refresh_search();
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }

        if self.help_visible {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.help_visible = false;
            }
            return;
        }

        if self.search_input.is_some() {
            self.handle_search_input(key);
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help_visible = true,
            KeyCode::Char('/') => self.search_input = Some(self.search_query.clone()),
            KeyCode::Char('n') => self.next_search_match(),
            KeyCode::Char('N') => self.previous_search_match(),
            KeyCode::Char('t') => self.sidebar_visible = !self.sidebar_visible,
            KeyCode::Tab => self.focus = toggle_focus(self.focus),
            KeyCode::Char(']') => self.switch_file(true),
            KeyCode::Char('[') => self.switch_file(false),
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => self.page_up(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => self.page_down(),
            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => self.scroll = usize::MAX,
            KeyCode::Enter if self.focus == Focus::Outline => self.jump_to_selected_heading(),
            _ => {}
        }
    }

    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.search_input = None,
            KeyCode::Enter => self.confirm_search(),
            KeyCode::Backspace => {
                if let Some(input) = self.search_input.as_mut() {
                    input.pop();
                }
            }
            KeyCode::Char(character) => {
                if let Some(input) = self.search_input.as_mut() {
                    input.push(character);
                }
            }
            _ => {}
        }
    }

    fn confirm_search(&mut self) {
        self.search_query = self.search_input.take().unwrap_or_default();
        self.refresh_search();
        self.search_selected = 0;
        if let Some(line) = self.search_matches.first() {
            self.scroll = *line;
            self.focus = Focus::Document;
        }
    }

    fn refresh_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_matches.clear();
            self.search_selected = 0;
            return;
        }

        let document_layout = layout::build(&self.document, self.document_width(), self.theme);
        self.search_matches = find_matches(&document_layout.lines, &self.search_query);
        if self.search_matches.is_empty() {
            self.search_selected = 0;
        } else {
            self.search_selected = self.search_selected.min(self.search_matches.len() - 1);
        }
    }

    fn next_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_selected = (self.search_selected + 1) % self.search_matches.len();
        self.scroll = self.search_matches[self.search_selected];
        self.focus = Focus::Document;
    }

    fn previous_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_selected = self
            .search_selected
            .checked_sub(1)
            .unwrap_or(self.search_matches.len() - 1);
        self.scroll = self.search_matches[self.search_selected];
        self.focus = Focus::Document;
    }

    pub fn search_result_position(&self) -> Option<(usize, usize)> {
        if self.search_matches.is_empty() {
            None
        } else {
            Some((self.search_selected + 1, self.search_matches.len()))
        }
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Outline => self.outline_selected = self.outline_selected.saturating_sub(1),
            Focus::Document => self.scroll = self.scroll.saturating_sub(1),
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            Focus::Outline => {
                if !self.document.outline.is_empty() {
                    self.outline_selected =
                        (self.outline_selected + 1).min(self.document.outline.len() - 1);
                }
            }
            Focus::Document => self.scroll = self.scroll.saturating_add(1),
        }
    }

    fn page_up(&mut self) {
        let amount = self.terminal_height.saturating_sub(5) as usize;
        self.scroll = self.scroll.saturating_sub(amount.max(1));
    }

    fn page_down(&mut self) {
        let amount = self.terminal_height.saturating_sub(5) as usize;
        self.scroll = self.scroll.saturating_add(amount.max(1));
    }

    fn jump_to_selected_heading(&mut self) {
        if self.document.outline.get(self.outline_selected).is_some() {
            let document_layout = layout::build(&self.document, self.document_width(), self.theme);
            if let Some(line) = document_layout.heading_line(self.outline_selected) {
                self.scroll = line;
            }
            self.focus = Focus::Document;
        }
    }

    fn switch_file(&mut self, next: bool) {
        if self.workspace.files.len() < 2 {
            return;
        }
        if next {
            self.workspace.next_file();
        } else {
            self.workspace.previous_file();
        }
        self.outline_selected = 0;
        self.scroll = 0;
        self.clear_search();
        self.load_active_file();
    }

    fn reload_if_changed(&mut self) {
        let current_modified = modified_time(self.workspace.active_path());
        if current_modified.is_some() && current_modified != self.last_modified {
            self.load_active_file();
        }
    }

    fn load_active_file(&mut self) {
        match self.workspace.reload_content() {
            Ok(content) => {
                self.document = Document::parse(&content);
                self.outline_selected = self
                    .outline_selected
                    .min(self.document.outline.len().saturating_sub(1));
                self.last_modified = modified_time(self.workspace.active_path());
                self.error = None;
                self.clear_search();
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_input = None;
        self.search_matches.clear();
        self.search_selected = 0;
    }
}

fn find_matches(lines: &[ratatui::text::Line<'static>], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            line.to_string()
                .to_lowercase()
                .contains(&query)
                .then_some(line_index)
        })
        .collect()
}

fn toggle_focus(focus: Focus) -> Focus {
    match focus {
        Focus::Outline => Focus::Document,
        Focus::Document => Focus::Outline,
    }
}

fn modified_time(path: Option<&Path>) -> Option<SystemTime> {
    path.and_then(|path| fs::metadata(path).ok()?.modified().ok())
}

#[cfg(test)]
mod tests {
    use ratatui::text::Line;

    use super::find_matches;

    #[test]
    fn finds_case_insensitive_matches_in_rendered_lines() {
        let lines = vec![
            Line::from("A calm document"),
            Line::from("Nothing here"),
            Line::from("A CALM ending"),
        ];

        assert_eq!(find_matches(&lines, "calm"), vec![0, 2]);
    }

    #[test]
    fn ignores_empty_search_queries() {
        let lines = vec![Line::from("A calm document")];

        assert!(find_matches(&lines, "").is_empty());
    }
}
