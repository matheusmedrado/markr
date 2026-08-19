use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_image::picker::Picker;

use crate::images::ImageStore;
use crate::layout;
use crate::markdown::Document;
use crate::theme::Theme;
use crate::workspace::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarPanel {
    Outline,
    Files,
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
    pub document_layout: layout::DocumentLayout,
    pub theme: Theme,
    pub images: ImageStore,
    pub focus: Focus,
    pub sidebar_panel: SidebarPanel,
    pub scroll: usize,
    pub outline_selected: usize,
    pub file_selected: usize,
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

#[derive(Debug, PartialEq, Eq)]
struct UiSnapshot {
    focus: Focus,
    sidebar_panel: SidebarPanel,
    scroll: usize,
    outline_selected: usize,
    file_selected: usize,
    workspace_selected: usize,
    sidebar_visible: bool,
    help_visible: bool,
    error: Option<String>,
    search_query: String,
    search_input: Option<String>,
    search_selected: usize,
    quit: bool,
}

impl App {
    pub fn new(workspace: Workspace, picker: Picker) -> Result<Self, Box<dyn std::error::Error>> {
        let document = Document::parse(&workspace.reload_content()?);
        let last_modified = modified_time(workspace.active_path());
        let document_dir = document_dir(&workspace);
        let mut images = ImageStore::new(picker);
        images.load(document_dir.as_deref(), &document);
        let theme = Theme::default();
        let terminal_width: u16 = 120;
        let document_width = terminal_width.saturating_sub(33).saturating_sub(5);
        let document_layout = layout::build(&document, document_width, theme, &images);
        Ok(Self {
            workspace,
            document,
            document_layout,
            theme,
            images,
            focus: Focus::Document,
            sidebar_panel: SidebarPanel::Outline,
            scroll: 0,
            outline_selected: 0,
            file_selected: 0,
            sidebar_visible: true,
            help_visible: false,
            terminal_width,
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
        let sidebar_width = if self.sidebar_visible {
            self.sidebar_width()
        } else {
            0
        };
        self.terminal_width
            .saturating_sub(sidebar_width)
            .saturating_sub(5)
    }

    fn document_height(&self) -> usize {
        usize::from(self.terminal_height.saturating_sub(3).max(1))
    }

    fn max_scroll(&self) -> usize {
        self.document_layout
            .lines
            .len()
            .saturating_sub(self.document_height())
    }

    fn rebuild_layout(&mut self) {
        self.document_layout = layout::build(
            &self.document,
            self.document_width(),
            self.theme,
            &self.images,
        );
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    pub fn sidebar_width(&self) -> u16 {
        33
    }

    pub fn update(&mut self, message: Message) -> bool {
        match message {
            Message::Key(key) => {
                let before = self.ui_snapshot();
                let previous_width = self.document_width();
                self.handle_key(key);
                if self.document_width() != previous_width {
                    self.rebuild_layout();
                }
                self.clamp_scroll();
                before != self.ui_snapshot()
            }
            Message::Tick => self.reload_if_changed(),
            Message::Resize { width, height } => {
                if self.terminal_width == width && self.terminal_height == height {
                    return false;
                }
                self.terminal_width = width;
                self.terminal_height = height;
                self.rebuild_layout();
                self.refresh_search();
                self.clamp_scroll();
                true
            }
        }
    }

    fn ui_snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            focus: self.focus,
            sidebar_panel: self.sidebar_panel,
            scroll: self.scroll,
            outline_selected: self.outline_selected,
            file_selected: self.file_selected,
            workspace_selected: self.workspace.selected,
            sidebar_visible: self.sidebar_visible,
            help_visible: self.help_visible,
            error: self.error.clone(),
            search_query: self.search_query.clone(),
            search_input: self.search_input.clone(),
            search_selected: self.search_selected,
            quit: self.quit,
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
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help_visible = true,
            KeyCode::Char('/') => self.search_input = Some(self.search_query.clone()),
            KeyCode::Char('n') => self.next_search_match(),
            KeyCode::Char('N') => self.previous_search_match(),
            KeyCode::Char('t') => self.sidebar_visible = !self.sidebar_visible,
            KeyCode::Char('1') => self.select_sidebar_panel(SidebarPanel::Outline),
            KeyCode::Char('2') => self.select_sidebar_panel(SidebarPanel::Files),
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
            KeyCode::Enter if self.focus == Focus::Sidebar => self.activate_sidebar_selection(),
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

        self.search_matches = find_matches(&self.document_layout.lines, &self.search_query);
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
            Focus::Sidebar => match self.sidebar_panel {
                SidebarPanel::Outline => {
                    self.outline_selected = self.outline_selected.saturating_sub(1)
                }
                SidebarPanel::Files => self.file_selected = self.file_selected.saturating_sub(1),
            },
            Focus::Document => self.scroll = self.scroll.saturating_sub(1),
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            Focus::Sidebar => match self.sidebar_panel {
                SidebarPanel::Outline => {
                    if !self.document.outline.is_empty() {
                        self.outline_selected =
                            (self.outline_selected + 1).min(self.document.outline.len() - 1);
                    }
                }
                SidebarPanel::Files => {
                    if !self.workspace.files.is_empty() {
                        self.file_selected =
                            (self.file_selected + 1).min(self.workspace.files.len() - 1);
                    }
                }
            },
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
            if let Some(line) = self.document_layout.heading_line(self.outline_selected) {
                self.scroll = line;
            }
            self.focus = Focus::Document;
        }
    }

    fn select_sidebar_panel(&mut self, panel: SidebarPanel) {
        self.sidebar_panel = panel;
        self.sidebar_visible = true;
        self.focus = Focus::Sidebar;
    }

    fn activate_sidebar_selection(&mut self) {
        match self.sidebar_panel {
            SidebarPanel::Outline => self.jump_to_selected_heading(),
            SidebarPanel::Files => self.open_selected_file(),
        }
    }

    fn open_selected_file(&mut self) {
        if self.workspace.files.get(self.file_selected).is_none() {
            return;
        }
        self.workspace.selected = self.file_selected;
        self.outline_selected = 0;
        self.scroll = 0;
        self.clear_search();
        self.load_active_file();
        self.focus = Focus::Document;
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
        self.file_selected = self.workspace.selected;
        self.outline_selected = 0;
        self.scroll = 0;
        self.clear_search();
        self.load_active_file();
    }

    fn reload_if_changed(&mut self) -> bool {
        let current_modified = modified_time(self.workspace.active_path());
        if current_modified.is_some() && current_modified != self.last_modified {
            self.load_active_file();
            true
        } else {
            false
        }
    }

    fn load_active_file(&mut self) {
        match self.workspace.reload_content() {
            Ok(content) => {
                self.document = Document::parse(&content);
                self.outline_selected = self
                    .outline_selected
                    .min(self.document.outline.len().saturating_sub(1));
                self.file_selected = self.workspace.selected;
                self.last_modified = modified_time(self.workspace.active_path());
                self.error = None;
                let dir = document_dir(&self.workspace);
                self.images.load(dir.as_deref(), &self.document);
                self.rebuild_layout();
                self.clear_search();
                self.clamp_scroll();
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

fn document_dir(workspace: &Workspace) -> Option<PathBuf> {
    workspace
        .active_path()
        .and_then(|path| path.parent())
        .map(Path::to_path_buf)
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
        Focus::Sidebar => Focus::Document,
        Focus::Document => Focus::Sidebar,
    }
}

fn modified_time(path: Option<&Path>) -> Option<SystemTime> {
    path.and_then(|path| fs::metadata(path).ok()?.modified().ok())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::text::Line;
    use ratatui_image::picker::Picker;

    use super::{App, Message, find_matches};
    use crate::workspace::Workspace;

    fn readme_app() -> App {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        App::new(workspace, Picker::halfblocks()).expect("app")
    }

    fn key(code: KeyCode) -> Message {
        Message::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

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

    #[test]
    fn ignores_scroll_attempts_beyond_document_boundaries() {
        let mut app = readme_app();

        assert!(!app.update(key(KeyCode::Up)));
        assert_eq!(app.scroll, 0);

        assert!(app.update(key(KeyCode::Char('G'))));
        let bottom = app.scroll;
        assert!(!app.update(key(KeyCode::Down)));
        assert_eq!(app.scroll, bottom);
    }

    #[test]
    fn escape_does_not_quit_the_document_view() {
        let mut app = readme_app();

        assert!(!app.update(key(KeyCode::Esc)));
        assert!(!app.should_quit());
        assert!(app.update(key(KeyCode::Char('q'))));
        assert!(app.should_quit());
    }
}
