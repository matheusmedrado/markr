use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
            last_modified,
            quit: false,
        })
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Key(key) => self.handle_key(key),
            Message::Tick => self.reload_if_changed(),
            Message::Resize { width, height } => {
                self.terminal_width = width;
                self.terminal_height = height;
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

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help_visible = true,
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
        if let Some(heading) = self.document.outline.get(self.outline_selected) {
            self.scroll = heading.block_index.saturating_mul(2);
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
                self.last_modified = modified_time(self.workspace.active_path());
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
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
