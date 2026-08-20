use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_image::picker::Picker;

use crate::explorer::{Activation, FileExplorer};
use crate::images::ImageStore;
use crate::layout;
use crate::markdown::Document;
use crate::theme::{Theme, ThemeName};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponsiveMode {
    Attached,
    Overlay,
    Fullscreen,
}

#[derive(Clone, Copy, Debug)]
struct Transition {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl Transition {
    fn new(value: f32, to: f32, started: Instant, duration: Duration) -> Self {
        Self {
            from: value,
            to,
            started,
            duration,
        }
    }

    fn value(self, at: Instant) -> f32 {
        if self.duration.is_zero() {
            return self.to;
        }
        let elapsed = at.saturating_duration_since(self.started);
        let progress = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).min(1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        self.from + (self.to - self.from) * eased
    }

    fn active(self, at: Instant) -> bool {
        at.saturating_duration_since(self.started) < self.duration
    }
}

#[derive(Debug)]
pub enum Message {
    Key {
        key: KeyEvent,
        at: Instant,
    },
    Tick,
    Resize {
        width: u16,
        height: u16,
        at: Instant,
    },
    Frame {
        at: Instant,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub line: usize,
    pub range: Range<usize>,
}

#[derive(Debug)]
pub struct App {
    pub workspace: Workspace,
    pub document: Document,
    pub document_layout: layout::DocumentLayout,
    pub theme: Theme,
    pub images: ImageStore,
    pub file_explorer: FileExplorer,
    pub focus: Focus,
    pub sidebar_panel: SidebarPanel,
    pub scroll: usize,
    pub outline_selected: usize,
    pub sidebar_visible: bool,
    pub help_visible: bool,
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub error: Option<String>,
    pub search_query: String,
    pub search_input: Option<String>,
    search_matches: Vec<SearchMatch>,
    search_selected: usize,
    responsive_mode: ResponsiveMode,
    sidebar_transition: Transition,
    tab_transition: Transition,
    selection_transition: Transition,
    help_transition: Transition,
    initial_started: Instant,
    temporary_message: Option<(String, Instant)>,
    message_started: Option<Instant>,
    last_modified: Option<SystemTime>,
    quit: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct UiSnapshot {
    theme: ThemeName,
    focus: Focus,
    sidebar_panel: SidebarPanel,
    scroll: usize,
    outline_selected: usize,
    workspace_selected: usize,
    explorer_directory: PathBuf,
    explorer_selected: usize,
    explorer_generation: u64,
    sidebar_visible: bool,
    help_visible: bool,
    error: Option<String>,
    search_query: String,
    search_input: Option<String>,
    search_selected: usize,
    quit: bool,
}

impl App {
    pub fn new(
        workspace: Workspace,
        picker: Picker,
        theme: Theme,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let file_explorer = FileExplorer::open(workspace.explorer_start_directory()?)?;
        let document = Document::parse(&workspace.reload_content()?);
        let last_modified = modified_time(workspace.active_path());
        let document_dir = document_dir(&workspace);
        let mut images = ImageStore::new(picker);
        images.load(document_dir.as_deref(), &document);
        let initial_started = Instant::now();
        let terminal_width: u16 = 120;
        let document_width = terminal_width.saturating_sub(33).saturating_sub(5);
        let document_layout = layout::build(&document, document_width, theme, &images);
        Ok(Self {
            workspace,
            document,
            document_layout,
            theme,
            images,
            file_explorer,
            focus: Focus::Document,
            sidebar_panel: SidebarPanel::Outline,
            scroll: 0,
            outline_selected: 0,
            sidebar_visible: true,
            help_visible: false,
            terminal_width,
            terminal_height: 40,
            error: None,
            search_query: String::new(),
            search_input: None,
            search_matches: Vec::new(),
            search_selected: 0,
            responsive_mode: ResponsiveMode::Attached,
            sidebar_transition: Transition::new(
                0.0,
                1.0,
                initial_started,
                Duration::from_millis(200),
            ),
            tab_transition: Transition::new(0.0, 1.0, initial_started, Duration::from_millis(120)),
            selection_transition: Transition::new(
                0.0,
                1.0,
                initial_started,
                Duration::from_millis(90),
            ),
            help_transition: Transition::new(0.0, 0.0, initial_started, Duration::ZERO),
            initial_started,
            temporary_message: None,
            message_started: None,
            last_modified,
            quit: false,
        })
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn document_width(&self) -> u16 {
        let reader_width = self.reader_outer_width();
        reader_width.saturating_sub(5)
    }

    fn document_height(&self) -> usize {
        usize::from(self.terminal_height.saturating_sub(4).max(1))
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
        30
    }

    pub fn responsive_mode(&self) -> ResponsiveMode {
        self.responsive_mode
    }

    pub fn sidebar_progress(&self, at: Instant) -> f32 {
        self.sidebar_transition.value(at)
    }

    pub fn help_progress(&self, at: Instant) -> f32 {
        self.help_transition.value(at)
    }

    pub fn animation_active(&self, at: Instant) -> bool {
        at.saturating_duration_since(self.initial_started) < Duration::from_millis(220)
            || self.sidebar_transition.active(at)
            || self.tab_transition.active(at)
            || self.selection_transition.active(at)
            || self.help_transition.active(at)
            || self.message_animation_active(at)
    }

    pub fn temporary_message(&self, at: Instant) -> Option<&str> {
        self.temporary_message
            .as_ref()
            .filter(|(_, expires)| *expires > at)
            .map(|(message, _)| message.as_str())
    }

    fn reader_outer_width(&self) -> u16 {
        let gutter = if self.terminal_width < 72 { 0 } else { 1 };
        match self.responsive_mode {
            ResponsiveMode::Attached if self.sidebar_visible => self
                .terminal_width
                .saturating_sub(self.sidebar_width())
                .saturating_sub(1)
                .saturating_sub(gutter * 2),
            ResponsiveMode::Fullscreen => 0,
            _ => self.terminal_width.saturating_sub(gutter * 2),
        }
    }

    fn set_sidebar_visible(&mut self, visible: bool, at: Instant) {
        if self.sidebar_visible == visible {
            return;
        }
        let current = self.sidebar_progress(at);
        self.sidebar_visible = visible;
        self.sidebar_transition = Transition::new(
            current,
            if visible { 1.0 } else { 0.0 },
            at,
            Duration::from_millis(160),
        );
    }

    fn set_help_visible(&mut self, visible: bool, at: Instant) {
        let current = self.help_progress(at);
        self.help_visible = visible;
        self.help_transition = Transition::new(
            current,
            if visible { 1.0 } else { 0.0 },
            at,
            Duration::from_millis(140),
        );
    }

    fn set_message(&mut self, message: impl Into<String>, at: Instant) {
        self.temporary_message = Some((message.into(), at + Duration::from_millis(1_600)));
        self.message_started = Some(at);
    }

    fn message_animation_active(&self, at: Instant) -> bool {
        let Some(started) = self.message_started else {
            return false;
        };
        let Some((_, expires)) = &self.temporary_message else {
            return false;
        };
        let entering = at < started + Duration::from_millis(150);
        let exit_started = (*expires)
            .checked_sub(Duration::from_millis(180))
            .unwrap_or(*expires);
        let exiting = at >= exit_started && at < *expires;
        entering || exiting
    }

    fn update_responsive_mode(&mut self, at: Instant) {
        let mode = match self.terminal_width {
            0..=71 => ResponsiveMode::Fullscreen,
            72..=99 => ResponsiveMode::Overlay,
            _ => ResponsiveMode::Attached,
        };
        if mode != self.responsive_mode {
            self.responsive_mode = mode;
            if mode == ResponsiveMode::Fullscreen && self.focus == Focus::Document {
                self.set_sidebar_visible(false, at);
            }
        }
    }

    pub fn update(&mut self, message: Message) -> bool {
        match message {
            Message::Key { key, at } => {
                let before = self.ui_snapshot();
                let previous_width = self.document_width();
                let previous_theme = self.theme;
                self.handle_key(key, at);
                if self.document_width() != previous_width || self.theme != previous_theme {
                    self.rebuild_layout();
                }
                self.clamp_scroll();
                before != self.ui_snapshot()
            }
            Message::Tick => self.reload_if_changed(),
            Message::Resize { width, height, at } => {
                if self.terminal_width == width && self.terminal_height == height {
                    return false;
                }
                self.terminal_width = width;
                self.terminal_height = height;
                self.update_responsive_mode(at);
                self.rebuild_layout();
                self.refresh_search();
                self.clamp_scroll();
                true
            }
            Message::Frame { at } => {
                let was_active = self.animation_active(at);
                if self
                    .temporary_message
                    .as_ref()
                    .is_some_and(|(_, expires)| *expires <= at)
                {
                    self.temporary_message = None;
                    self.message_started = None;
                }
                was_active
            }
        }
    }

    fn ui_snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            theme: self.theme.name,
            focus: self.focus,
            sidebar_panel: self.sidebar_panel,
            scroll: self.scroll,
            outline_selected: self.outline_selected,
            workspace_selected: self.workspace.selected,
            explorer_directory: self.file_explorer.directory().to_path_buf(),
            explorer_selected: self.file_explorer.selected(),
            explorer_generation: self.file_explorer.generation(),
            sidebar_visible: self.sidebar_visible,
            help_visible: self.help_visible,
            error: self.error.clone(),
            search_query: self.search_query.clone(),
            search_input: self.search_input.clone(),
            search_selected: self.search_selected,
            quit: self.quit,
        }
    }

    fn handle_key(&mut self, key: KeyEvent, at: Instant) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }

        if self.help_visible {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.set_help_visible(false, at);
            }
            return;
        }

        if self.search_input.is_some() {
            self.handle_search_input(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.set_help_visible(true, at),
            KeyCode::Char('/') => self.search_input = Some(self.search_query.clone()),
            KeyCode::Char('n') => self.next_search_match(),
            KeyCode::Char('N') => self.previous_search_match(),
            KeyCode::Char('t') => self.set_sidebar_visible(!self.sidebar_visible, at),
            KeyCode::Char('T') => {
                self.theme = self.theme.next();
                self.set_message(format!("theme: {}", self.theme.name), at);
            }
            KeyCode::Char('1') => self.select_sidebar_panel(SidebarPanel::Outline, at),
            KeyCode::Char('2') => self.select_sidebar_panel(SidebarPanel::Files, at),
            KeyCode::Tab => {
                self.focus = toggle_focus(self.focus);
                self.tab_transition = Transition::new(0.0, 1.0, at, Duration::from_millis(120));
                if self.focus == Focus::Sidebar {
                    self.set_sidebar_visible(true, at);
                } else if self.responsive_mode != ResponsiveMode::Attached {
                    self.set_sidebar_visible(false, at);
                }
            }
            KeyCode::Left if self.focus == Focus::Sidebar => {
                self.select_sidebar_panel(SidebarPanel::Outline, at)
            }
            KeyCode::Right if self.focus == Focus::Sidebar => {
                self.select_sidebar_panel(SidebarPanel::Files, at)
            }
            KeyCode::Char('h') | KeyCode::Backspace
                if self.focus == Focus::Sidebar && self.sidebar_panel == SidebarPanel::Files =>
            {
                self.browse_parent(at)
            }
            KeyCode::Char('l')
                if self.focus == Focus::Sidebar && self.sidebar_panel == SidebarPanel::Files =>
            {
                self.activate_explorer_entry(at)
            }
            KeyCode::Char('r')
                if self.focus == Focus::Sidebar && self.sidebar_panel == SidebarPanel::Files =>
            {
                self.refresh_explorer(at)
            }
            KeyCode::Char(']') => self.switch_file(true),
            KeyCode::Char('[') => self.switch_file(false),
            KeyCode::Up | KeyCode::Char('k') => self.move_up(at),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(at),
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => self.page_up(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => self.page_down(),
            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => self.scroll = usize::MAX,
            KeyCode::Enter if self.focus == Focus::Sidebar => self.activate_sidebar_selection(at),
            KeyCode::Esc => {
                self.set_help_visible(false, at);
                self.search_input = None;
                if self.focus == Focus::Sidebar {
                    self.focus = Focus::Document;
                    if self.responsive_mode != ResponsiveMode::Attached {
                        self.set_sidebar_visible(false, at);
                    }
                }
            }
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
        if let Some(search_match) = self.search_matches.first() {
            self.scroll = search_match.line;
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
        self.scroll = self.search_matches[self.search_selected].line;
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
        self.scroll = self.search_matches[self.search_selected].line;
        self.focus = Focus::Document;
    }

    pub fn search_result_position(&self) -> Option<(usize, usize)> {
        if self.search_matches.is_empty() {
            None
        } else {
            Some((self.search_selected + 1, self.search_matches.len()))
        }
    }

    pub fn search_matches_on_line(
        &self,
        line: usize,
    ) -> impl Iterator<Item = (usize, &SearchMatch)> {
        let start = self
            .search_matches
            .partition_point(|search_match| search_match.line < line);
        let end = self
            .search_matches
            .partition_point(|search_match| search_match.line <= line);
        self.search_matches[start..end]
            .iter()
            .enumerate()
            .map(move |(offset, search_match)| (start + offset, search_match))
    }

    pub fn selected_search_match(&self) -> Option<usize> {
        (!self.search_matches.is_empty()).then_some(self.search_selected)
    }

    fn move_up(&mut self, at: Instant) {
        match self.focus {
            Focus::Sidebar => match self.sidebar_panel {
                SidebarPanel::Outline => {
                    self.outline_selected = self.outline_selected.saturating_sub(1)
                }
                SidebarPanel::Files => self.file_explorer.move_up(),
            },
            Focus::Document => self.scroll = self.scroll.saturating_sub(1),
        }
        self.selection_transition = Transition::new(0.0, 1.0, at, Duration::from_millis(90));
    }

    fn move_down(&mut self, at: Instant) {
        match self.focus {
            Focus::Sidebar => match self.sidebar_panel {
                SidebarPanel::Outline => {
                    if !self.document.outline.is_empty() {
                        self.outline_selected =
                            (self.outline_selected + 1).min(self.document.outline.len() - 1);
                    }
                }
                SidebarPanel::Files => {
                    self.file_explorer.move_down();
                }
            },
            Focus::Document => self.scroll = self.scroll.saturating_add(1),
        }
        self.selection_transition = Transition::new(0.0, 1.0, at, Duration::from_millis(90));
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

    fn select_sidebar_panel(&mut self, panel: SidebarPanel, at: Instant) {
        self.sidebar_panel = panel;
        self.set_sidebar_visible(true, at);
        self.tab_transition = Transition::new(0.0, 1.0, at, Duration::from_millis(120));
        self.focus = Focus::Sidebar;
    }

    fn activate_sidebar_selection(&mut self, at: Instant) {
        match self.sidebar_panel {
            SidebarPanel::Outline => self.jump_to_selected_heading(),
            SidebarPanel::Files => self.activate_explorer_entry(at),
        }
    }

    fn activate_explorer_entry(&mut self, at: Instant) {
        match self.file_explorer.activate() {
            Ok(Activation::Navigated) => {
                self.error = None;
                self.set_message("directory opened", at);
            }
            Ok(Activation::OpenMarkdown(path)) => self.open_explorer_file(&path),
            Ok(Activation::Unsupported(path)) => {
                let message = format!(
                    "{} is not a Markdown document",
                    path.file_name()
                        .map(|name| name.to_string_lossy())
                        .unwrap_or_default()
                );
                self.error = Some(message.clone());
                self.set_message(message, at);
            }
            Err(error) => {
                let message = format!("Cannot open directory: {error}");
                self.error = Some(message.clone());
                self.set_message(message, at);
            }
        }
    }

    fn open_explorer_file(&mut self, path: &Path) {
        match self.workspace.open_file(path) {
            Ok(()) => {
                self.outline_selected = 0;
                self.scroll = 0;
                self.clear_search();
                self.load_active_file();
                self.focus = Focus::Document;
            }
            Err(error) => {
                let message = format!("Cannot open file: {error}");
                self.error = Some(message.clone());
                self.set_message(message, Instant::now());
            }
        }
    }

    fn browse_parent(&mut self, at: Instant) {
        match self.file_explorer.go_parent() {
            Ok(_) => {
                self.error = None;
                self.set_message("parent directory", at);
            }
            Err(error) => {
                let message = format!("Cannot open directory: {error}");
                self.error = Some(message.clone());
                self.set_message(message, at);
            }
        }
    }

    fn refresh_explorer(&mut self, at: Instant) {
        match self.file_explorer.refresh() {
            Ok(()) => {
                self.error = None;
                self.set_message("files reloaded", at);
            }
            Err(error) => {
                let message = format!("Cannot refresh directory: {error}");
                self.error = Some(message.clone());
                self.set_message(message, at);
            }
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

fn find_matches(lines: &[ratatui::text::Line<'static>], query: &str) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    lines
        .iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            find_case_insensitive_ranges(&line.to_string(), query)
                .into_iter()
                .map(move |range| SearchMatch {
                    line: line_index,
                    range,
                })
        })
        .collect()
}

fn find_case_insensitive_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let folded_query = query.to_lowercase();
    if folded_query.is_empty() {
        return Vec::new();
    }

    let mut folded_text = String::new();
    let mut segments = Vec::new();
    for (start, character) in text.char_indices() {
        let original = start..start + character.len_utf8();
        let folded_start = folded_text.len();
        folded_text.extend(character.to_lowercase());
        segments.push((folded_start..folded_text.len(), original));
    }

    folded_text
        .match_indices(&folded_query)
        .filter_map(|(start, matched)| {
            let end = start + matched.len();
            let original_start = segments
                .iter()
                .find(|(folded, _)| folded.start <= start && start < folded.end)?
                .1
                .start;
            let original_end = segments
                .iter()
                .find(|(folded, _)| folded.start < end && end <= folded.end)
                .or_else(|| segments.iter().rev().find(|(folded, _)| folded.start < end))?
                .1
                .end;
            Some(original_start..original_end)
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
    use std::time::Instant;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::text::Line;
    use ratatui_image::picker::Picker;

    use super::{App, Focus, Message, ResponsiveMode, SearchMatch, SidebarPanel, find_matches};
    use crate::theme::{Theme, ThemeName};
    use crate::workspace::Workspace;

    fn readme_app() -> App {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let workspace = Workspace::open(Some(path), true).expect("README workspace");
        App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app")
    }

    fn key(code: KeyCode) -> Message {
        Message::Key {
            key: KeyEvent::new(code, KeyModifiers::NONE),
            at: Instant::now(),
        }
    }

    #[test]
    fn finds_case_insensitive_matches_in_rendered_lines() {
        let lines = vec![
            Line::from("A calm document"),
            Line::from("Nothing here"),
            Line::from("A CALM ending"),
        ];

        assert_eq!(
            find_matches(&lines, "calm"),
            vec![
                SearchMatch {
                    line: 0,
                    range: 2..6,
                },
                SearchMatch {
                    line: 2,
                    range: 2..6,
                },
            ]
        );
    }

    #[test]
    fn finds_every_occurrence_on_the_same_line() {
        let lines = vec![Line::from("calm, CALM, calm")];

        assert_eq!(
            find_matches(&lines, "calm"),
            vec![
                SearchMatch {
                    line: 0,
                    range: 0..4,
                },
                SearchMatch {
                    line: 0,
                    range: 6..10,
                },
                SearchMatch {
                    line: 0,
                    range: 12..16,
                },
            ]
        );
    }

    #[test]
    fn keeps_unicode_match_ranges_aligned_with_the_original_text() {
        let lines = vec![Line::from("CAFÉ e café")];

        assert_eq!(
            find_matches(&lines, "café"),
            vec![
                SearchMatch {
                    line: 0,
                    range: 0..5,
                },
                SearchMatch {
                    line: 0,
                    range: 8..13,
                },
            ]
        );
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

    #[test]
    fn cycles_the_theme_and_rebuilds_the_document_layout() {
        let mut app = readme_app();
        let original_layout = app.document_layout.lines.clone();

        assert!(app.update(key(KeyCode::Char('T'))));

        assert_eq!(app.theme.name, ThemeName::Midnight);
        assert_ne!(app.document_layout.lines, original_layout);
    }

    #[test]
    fn classifies_the_three_responsive_layouts() {
        let mut app = readme_app();
        let now = Instant::now();

        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: now,
        });
        assert_eq!(app.responsive_mode(), ResponsiveMode::Attached);

        app.update(Message::Resize {
            width: 84,
            height: 30,
            at: now,
        });
        assert_eq!(app.responsive_mode(), ResponsiveMode::Overlay);

        app.update(Message::Resize {
            width: 60,
            height: 24,
            at: now,
        });
        assert_eq!(app.responsive_mode(), ResponsiveMode::Fullscreen);
    }

    #[test]
    fn arrows_switch_sidebar_panels_and_tab_closes_compact_sidebar() {
        let mut app = readme_app();
        let now = Instant::now();

        assert_eq!(app.focus, Focus::Document);
        app.update(Message::Key {
            key: KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            at: now,
        });
        assert_eq!(app.focus, Focus::Sidebar);
        app.update(Message::Key {
            key: KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            at: now,
        });
        assert_eq!(app.sidebar_panel, SidebarPanel::Files);
        app.update(Message::Resize {
            width: 84,
            height: 30,
            at: now,
        });
        app.update(Message::Key {
            key: KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            at: now,
        });
        assert_eq!(app.focus, Focus::Document);
        assert!(!app.sidebar_visible);
    }

    #[test]
    fn transition_can_be_sampled_and_retargeted_without_a_queue() {
        let start = Instant::now();
        let transition =
            super::Transition::new(0.0, 1.0, start, std::time::Duration::from_millis(100));
        let middle = transition.value(start + std::time::Duration::from_millis(50));
        assert!(middle > 0.0 && middle < 1.0);
        assert_eq!(
            transition.value(start + std::time::Duration::from_millis(100)),
            1.0
        );

        let retargeted = super::Transition::new(
            middle,
            0.0,
            start + std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(100),
        );
        assert_eq!(
            retargeted.value(start + std::time::Duration::from_millis(50)),
            middle
        );
        assert_eq!(
            retargeted.value(start + std::time::Duration::from_millis(200)),
            0.0
        );
    }

    #[test]
    fn animation_polling_stops_after_all_transitions_settle() {
        let app = readme_app();
        assert!(
            !app.animation_active(app.initial_started + std::time::Duration::from_millis(1_000))
        );
    }
}
