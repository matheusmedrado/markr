use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_image::picker::Picker;

use crate::editor::EditorBuffer;
use crate::explorer::{Activation, FileExplorer};
use crate::images::ImageStore;
use crate::layout;
use crate::markdown::Document;
use crate::selection::{CursorPosition, Selection};
use crate::syntax;
use crate::theme::{Theme, ThemeName};
use crate::workspace::Workspace;
use crate::wrap;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsavedAction {
    CancelEdit,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExternalPromptAction {
    Save,
    Finish(UnsavedAction),
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
    Mouse {
        event: crossterm::event::MouseEvent,
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
    /// The measure the currently decoded images were sized against.
    image_measure: usize,
    /// Lines the viewport still has to travel, signed and fractional. A wheel
    /// notch lands here rather than on `scroll`, and is paid off over time.
    scroll_debt: f32,
    /// When the glide was last advanced, so it travels by elapsed time rather
    /// than by frame count and keeps its pace when the frame rate wobbles.
    scroll_advanced: Option<Instant>,
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
    pub editor: Option<EditorBuffer>,
    /// The editor's Markdown highlighting, rebuilt when the buffer or the
    /// palette moves rather than on every draw.
    editor_highlight: syntax::HighlightCache,
    /// Where each source line falls once wrapped to the editor's width.
    editor_wrap: wrap::WrapLayout,
    /// The first visual row on screen. Rows, not source lines: a wrapped line
    /// can be scrolled into partway.
    pub editor_scroll: usize,
    /// The cell the cursor aims for while travelling up and down, so passing
    /// through a short row does not drag it permanently to the left.
    editor_goal_column: Option<usize>,
    pub unsaved_action: Option<UnsavedAction>,
    pub external_change_detected: bool,
    external_prompt: Option<ExternalPromptAction>,
    pub search_query: String,
    pub search_input: Option<String>,
    search_matches: Vec<SearchMatch>,
    search_selected: usize,
    pub selection: Option<Selection>,
    pub select_mode: bool,
    responsive_mode: ResponsiveMode,
    sidebar_transition: Transition,
    tab_transition: Transition,
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
    editor_text: Option<String>,
    editor_cursor: Option<CursorPosition>,
    editor_scroll: usize,
    editor_dirty: bool,
    unsaved_action: Option<UnsavedAction>,
    external_change_detected: bool,
    external_prompt: Option<ExternalPromptAction>,
    search_query: String,
    search_input: Option<String>,
    search_selected: usize,
    selection: Option<Selection>,
    select_mode: bool,
    quit: bool,
}

/// Lines covered by one notch of a discrete mouse wheel. A trackpad sends a
/// stream of events and covers ground by volume; a wheel sends one event per
/// detent, so a detent has to be worth enough that reading a page does not
/// mean spinning the wheel raw.
const MOUSE_WHEEL_LINES: f32 = 5.0;

/// How briskly a glide travels, in lines per second per line still owed. The
/// rate falls as the viewport nears its target, so the motion eases out.
const SCROLL_RATE_GAIN: f32 = 11.0;

/// The floor stops the eased tail from crawling, and sets the pace of a single
/// notch: five lines at fifty a second is a line every twenty milliseconds, so
/// a detent covers real ground while still reading as motion rather than a
/// jump, and a long flick comes to rest promptly instead of drifting.
const SCROLL_MIN_RATE: f32 = 50.0;

/// The ceiling keeps a long flick from outrunning the eye entirely.
const SCROLL_MAX_RATE: f32 = 420.0;

/// A frame gap longer than this is a stall — a resize, a reload — and advancing
/// a glide by the whole gap would make it jump.
const MAX_FRAME_GAP: Duration = Duration::from_millis(50);

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
        let initial_started = Instant::now();
        let terminal_width: u16 = 120;
        let document_width = terminal_width
            .saturating_sub(30)
            .saturating_sub(layout::RAIL_WIDTH);
        let mut images = ImageStore::new(picker);
        let image_measure = layout::measure_for(document_width);
        images.load(document_dir.as_deref(), &document, image_measure);
        let document_layout = layout::build(&document, document_width, theme, &images);
        Ok(Self {
            workspace,
            document,
            document_layout,
            image_measure,
            scroll_debt: 0.0,
            scroll_advanced: None,
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
            editor: None,
            editor_highlight: syntax::HighlightCache::default(),
            editor_wrap: wrap::WrapLayout::default(),
            editor_scroll: 0,
            editor_goal_column: None,
            unsaved_action: None,
            external_change_detected: false,
            external_prompt: None,
            search_query: String::new(),
            search_input: None,
            search_matches: Vec::new(),
            search_selected: 0,
            selection: None,
            select_mode: false,
            responsive_mode: ResponsiveMode::Attached,
            sidebar_transition: Transition::new(
                0.0,
                1.0,
                initial_started,
                Duration::from_millis(200),
            ),
            tab_transition: Transition::new(0.0, 1.0, initial_started, Duration::from_millis(120)),
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

    pub fn is_editing(&self) -> bool {
        self.editor.is_some()
    }

    pub fn editor_lines(&self) -> &[String] {
        self.editor.as_ref().map(EditorBuffer::lines).unwrap_or(&[])
    }

    pub fn editor_cursor(&self) -> Option<CursorPosition> {
        self.editor.as_ref().map(EditorBuffer::cursor)
    }

    /// The highlighted form of [`Self::editor_lines`], one entry per line.
    pub fn editor_highlight(&self) -> &[Vec<ratatui::text::Span<'static>>] {
        self.editor_highlight.lines()
    }

    /// Where each source line falls on screen once wrapped.
    pub fn editor_wrap(&self) -> &wrap::WrapLayout {
        &self.editor_wrap
    }

    pub fn editor_dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(EditorBuffer::dirty)
    }

    pub fn has_unsaved_prompt(&self) -> bool {
        self.unsaved_action.is_some()
    }

    pub fn has_external_prompt(&self) -> bool {
        self.external_prompt.is_some()
    }

    pub fn editor_text(&self) -> Option<String> {
        self.editor.as_ref().map(EditorBuffer::text)
    }

    /// The columns left for text once the line-number gutter is taken out.
    /// The gutter is `"  N │ "`: two spaces, the number, then three more.
    pub fn editor_content_width(&self) -> usize {
        usize::from(self.document_width())
            .saturating_sub(self.editor_gutter_width())
            .max(1)
    }

    pub fn editor_gutter_width(&self) -> usize {
        self.editor_lines().len().max(1).to_string().len() + 5
    }

    pub fn document_width(&self) -> u16 {
        self.reader_outer_width().saturating_sub(layout::RAIL_WIDTH)
    }

    /// The reader owns everything between the header, its spacer row and the
    /// status bar.
    fn document_height(&self) -> usize {
        usize::from(self.terminal_height.saturating_sub(3).max(1))
    }

    /// How far through the document the viewport sits, as a percentage.
    pub fn reading_progress(&self) -> usize {
        let total = self.document_layout.lines.len();
        let height = self.document_height();
        if total <= height {
            return 100;
        }
        let span = total.saturating_sub(height);
        self.scroll.min(span).saturating_mul(100) / span
    }

    fn max_scroll(&self) -> usize {
        self.document_layout
            .lines
            .len()
            .saturating_sub(self.document_height())
    }

    /// Re-decodes the document's images at the reader's current measure.
    fn reload_images(&mut self) {
        let dir = document_dir(&self.workspace);
        self.image_measure = layout::measure_for(self.document_width());
        self.images
            .load(dir.as_deref(), &self.document, self.image_measure);
    }

    fn rebuild_layout(&mut self) {
        // Images are sized in cells at decode time, so a terminal resize that
        // moves the measure has to resize them too.
        if layout::measure_for(self.document_width()) != self.image_measure {
            self.reload_images();
        }
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
            || self.help_transition.active(at)
            || self.message_animation_active(at)
            || self.gliding()
    }

    pub fn temporary_message(&self, at: Instant) -> Option<&str> {
        self.temporary_message
            .as_ref()
            .filter(|(_, expires)| *expires > at)
            .map(|(message, _)| message.as_str())
    }

    fn reader_outer_width(&self) -> u16 {
        match self.responsive_mode {
            ResponsiveMode::Attached if self.sidebar_visible => {
                self.terminal_width.saturating_sub(self.sidebar_width())
            }
            ResponsiveMode::Fullscreen => 0,
            _ => self.terminal_width,
        }
    }

    pub fn reader_outer_area(&self) -> ratatui::layout::Rect {
        // Row 0 is the header, row 1 the spacer, the last row the status bar.
        let total = ratatui::layout::Rect::new(
            0,
            2,
            self.terminal_width,
            self.terminal_height.saturating_sub(3),
        );
        let x = if self.responsive_mode == ResponsiveMode::Attached && self.sidebar_visible {
            total.x.saturating_add(self.sidebar_width())
        } else {
            total.x
        };
        let width = total.width.saturating_sub(x.saturating_sub(total.x));
        ratatui::layout::Rect::new(x, total.y, width, total.height)
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
        let cursor_before = self.editor_cursor();
        let changed = self.apply(message);

        // The wrap has to be rebuilt before the viewport is settled against
        // it, or an edit that moved where the lines break would be chased
        // with a scroll computed from where they used to.
        let rewrapped = self.sync_editor_view();

        // Only follow the cursor when the cursor or the wrapping actually
        // moved. The wheel deliberately leaves the cursor behind, and
        // dragging the view back to it would make the editor impossible to
        // look around in.
        if rewrapped || self.editor_cursor() != cursor_before {
            self.ensure_editor_cursor_visible();
        }
        changed
    }

    /// Rebuilds the editor's highlighting and wrapping if the buffer, the
    /// palette or the width has moved since they were last built. Cheap
    /// enough to call after every message: the common case is two
    /// comparisons.
    ///
    /// Returns whether the wrapping was rebuilt, which is what tells the
    /// caller the viewport may need settling against it again.
    fn sync_editor_view(&mut self) -> bool {
        let Some(revision) = self.editor.as_ref().map(EditorBuffer::revision) else {
            self.editor_highlight.clear();
            self.editor_wrap.clear();
            return false;
        };

        if !self.editor_highlight.is_current(revision, self.theme.name)
            && let Some(text) = self.editor_text()
        {
            self.editor_highlight.rebuild(revision, self.theme, &text);
        }

        let width = self.editor_content_width();
        if self.editor_wrap.is_current(revision, width) {
            return false;
        }
        if let Some(editor) = self.editor.as_ref() {
            self.editor_wrap.rebuild(revision, width, editor.lines());
            return true;
        }
        false
    }

    /// Moves the cursor `rows` visual rows, keeping the cell it is aiming for
    /// so a short row passed through on the way does not capture it.
    fn editor_move_rows(&mut self, rows: isize) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        if self.editor_wrap.is_empty() {
            return;
        }

        let lines = editor.lines();
        let (row, column) = self.editor_wrap.locate(editor.cursor(), lines);
        let goal = self.editor_goal_column.unwrap_or(column);
        let last = self.editor_wrap.len().saturating_sub(1);
        let target = if rows < 0 {
            row.saturating_sub(rows.unsigned_abs())
        } else {
            row.saturating_add(rows as usize).min(last)
        };
        let position = self.editor_wrap.position_at(target, goal, lines);

        if let Some(editor) = self.editor.as_mut() {
            editor.set_cursor(position);
        }
        // `set_cursor` cannot know this was a vertical move, so the aim is
        // restored after it rather than before.
        self.editor_goal_column = Some(goal);
    }

    /// Moves the cursor to the start or the end of the visual row it is on.
    fn editor_move_to_row_edge(&mut self, to_end: bool) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let lines = editor.lines();
        let (index, _) = self.editor_wrap.locate(editor.cursor(), lines);
        let Some(row) = self.editor_wrap.rows().get(index).copied() else {
            return;
        };

        let column = if !to_end {
            row.start
        } else if self.editor_wrap.ends_line(index) {
            row.end
        } else {
            // A wrapped row breaks after a space. Landing on that space would
            // put the caret at the start of the next row, so the end of this
            // row is the last thing actually printed on it.
            self.editor_wrap.last_printed_column(index, lines)
        };

        self.set_editor_cursor(CursorPosition::new(row.line, column));
    }

    fn apply(&mut self, message: Message) -> bool {
        match message {
            Message::Key { key, at } => {
                let before = self.ui_snapshot();
                let previous_width = self.document_width();
                let previous_theme = self.theme;
                self.handle_key(key, at);
                if self.document_width() != previous_width || self.theme != previous_theme {
                    self.rebuild_layout();
                }
                self.clamp_selection();
                self.clamp_scroll();
                before != self.ui_snapshot()
            }
            // Mouse capture delivers motion events too. Redrawing on every
            // one of them kept the reader repainting whenever the pointer
            // crossed it, for nothing.
            Message::Mouse { event } => self.handle_mouse(event),
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
                self.clamp_selection();
                self.clamp_scroll();
                self.ensure_editor_cursor_visible();
                true
            }
            Message::Frame { at } => {
                let glided = self.advance_scroll(at);
                let was_active = self.animation_active(at) || glided;
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
            editor_text: self.editor.as_ref().map(EditorBuffer::text),
            editor_cursor: self.editor_cursor(),
            editor_scroll: self.editor_scroll,
            editor_dirty: self.editor_dirty(),
            unsaved_action: self.unsaved_action,
            external_change_detected: self.external_change_detected,
            external_prompt: self.external_prompt,
            search_query: self.search_query.clone(),
            search_input: self.search_input.clone(),
            search_selected: self.search_selected,
            selection: self.selection.clone(),
            select_mode: self.select_mode,
            quit: self.quit,
        }
    }

    fn handle_key(&mut self, key: KeyEvent, at: Instant) {
        if self.external_prompt.is_some() {
            self.handle_external_prompt(key, at);
            return;
        }

        if self.unsaved_action.is_some() {
            self.handle_unsaved_prompt(key, at);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.request_quit(at);
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

        if self.is_editing() {
            self.handle_editor_key(key, at);
            return;
        }

        match key.code {
            KeyCode::Char('e') if self.focus == Focus::Document => self.enter_editor(at),
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.set_help_visible(true, at),
            KeyCode::Char('/') => self.search_input = Some(self.search_query.clone()),
            KeyCode::Char('n') => self.next_search_match(),
            KeyCode::Char('N') => self.previous_search_match(),
            KeyCode::Char('v') if self.focus == Focus::Document => {
                self.start_selection(CursorPosition::new(self.scroll, 0))
            }
            KeyCode::Char('y') if self.focus == Focus::Document => {
                let _ = self.copy_selection();
                self.clear_selection();
            }
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
                    self.clear_selection();
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
            KeyCode::Up | KeyCode::Char('k') => {
                if self.select_mode && self.focus == Focus::Document {
                    self.selection_move_up();
                } else {
                    self.move_up();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.select_mode && self.focus == Focus::Document {
                    self.selection_move_down();
                } else {
                    self.move_down();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.select_mode && self.focus == Focus::Document {
                    self.selection_move_left();
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.select_mode && self.focus == Focus::Document {
                    self.selection_move_right();
                }
            }
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => self.page_up(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => self.page_down(),
            KeyCode::Char('g') => self.jump_scroll(0),
            KeyCode::Char('G') => self.jump_scroll(usize::MAX),
            KeyCode::Enter if self.focus == Focus::Sidebar => self.activate_sidebar_selection(at),
            KeyCode::Esc => {
                self.set_help_visible(false, at);
                if self.select_mode && self.focus == Focus::Document {
                    self.clear_selection();
                } else if self.search_input.is_some() || !self.search_query.is_empty() {
                    self.cancel_search();
                } else if self.focus == Focus::Sidebar {
                    self.focus = Focus::Document;
                    if self.responsive_mode != ResponsiveMode::Attached {
                        self.set_sidebar_visible(false, at);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent, at: Instant) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            if self.external_change_detected {
                self.open_external_prompt(ExternalPromptAction::Save, at);
            } else {
                self.save_editor(at);
            }
            return;
        }

        // Anything but travelling up and down gives up the cell the cursor
        // was aiming for, so the next vertical move starts from where it
        // actually is.
        let vertical = matches!(
            key.code,
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
        ) || (key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('u') | KeyCode::Char('d')));
        if !vertical {
            self.editor_goal_column = None;
        }

        let page_amount = self.document_height().saturating_sub(1).max(1);
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('z') => {
                    self.editor_move(|editor| editor.undo());
                    self.ensure_editor_cursor_visible();
                    return;
                }
                KeyCode::Char('y') => {
                    self.editor_move(|editor| editor.redo());
                    self.ensure_editor_cursor_visible();
                    return;
                }
                KeyCode::Char('u') => {
                    self.editor_move_rows(-(page_amount as isize));
                    return;
                }
                KeyCode::Char('d') => {
                    self.editor_move_rows(page_amount as isize);
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            // No bare letter quits the editor. `q` is a letter people write,
            // and a reader's shortcut has no business reaching a text field.
            // Esc leaves, and quitting is the reader's business.
            KeyCode::Esc => self.request_cancel_editor(at),
            KeyCode::Left => self.editor_move(|editor| editor.move_left()),
            KeyCode::Right => self.editor_move(|editor| editor.move_right()),
            // Up and down travel by what is on screen, not by what is in the
            // file: a wrapped line is several rows, and skipping all of them
            // at once would make a long paragraph impossible to move through.
            KeyCode::Up => self.editor_move_rows(-1),
            KeyCode::Down => self.editor_move_rows(1),
            KeyCode::Home => self.editor_move_to_row_edge(false),
            KeyCode::End => self.editor_move_to_row_edge(true),
            KeyCode::PageUp => self.editor_move_rows(-(page_amount as isize)),
            KeyCode::PageDown => self.editor_move_rows(page_amount as isize),
            KeyCode::Backspace => self.editor_move(|editor| editor.backspace()),
            KeyCode::Delete => self.editor_move(|editor| editor.delete()),
            KeyCode::Enter => self.editor_move(|editor| editor.insert('\n')),
            KeyCode::Tab => self.editor_move(|editor| {
                for _ in 0..4 {
                    editor.insert(' ');
                }
            }),
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.editor_move(|editor| editor.insert(character));
            }
            _ => {}
        }
        self.ensure_editor_cursor_visible();
    }

    fn handle_unsaved_prompt(&mut self, key: KeyEvent, at: Instant) {
        let Some(action) = self.unsaved_action else {
            return;
        };

        match key.code {
            KeyCode::Char('s') | KeyCode::Enter => {
                if self.external_change_detected {
                    self.unsaved_action = None;
                    self.open_external_prompt(ExternalPromptAction::Finish(action), at);
                } else if self.save_editor(at) {
                    self.unsaved_action = None;
                    self.finish_editor_action(action, at);
                }
            }
            KeyCode::Char('d') => {
                self.unsaved_action = None;
                self.discard_editor(at, action);
            }
            KeyCode::Esc | KeyCode::Char('c') => {
                self.unsaved_action = None;
                self.set_message("continuing edit", at);
            }
            _ => {}
        }
    }

    fn handle_external_prompt(&mut self, key: KeyEvent, at: Instant) {
        let Some(action) = self.external_prompt else {
            return;
        };

        match key.code {
            KeyCode::Char('o') | KeyCode::Enter => {
                if self.save_editor(at) {
                    self.external_prompt = None;
                    self.finish_external_action(action, at);
                }
            }
            KeyCode::Char('r') => {
                if self.reload_editor_from_disk() {
                    self.external_prompt = None;
                    self.finish_external_action(action, at);
                }
            }
            KeyCode::Esc => {
                self.external_prompt = None;
                if let ExternalPromptAction::Finish(unsaved_action) = action {
                    self.unsaved_action = Some(unsaved_action);
                    self.set_message("unsaved changes · s save · d discard · Esc continue", at);
                } else {
                    self.set_message("save cancelled", at);
                }
            }
            _ => {}
        }
    }

    fn open_external_prompt(&mut self, action: ExternalPromptAction, at: Instant) {
        self.external_prompt = Some(action);
        self.set_message(
            "file changed on disk · o overwrite · r reload · Esc continue",
            at,
        );
    }

    fn finish_external_action(&mut self, action: ExternalPromptAction, at: Instant) {
        if let ExternalPromptAction::Finish(unsaved_action) = action {
            self.finish_editor_action(unsaved_action, at);
        }
    }

    fn request_cancel_editor(&mut self, at: Instant) {
        if self.editor_dirty() {
            self.unsaved_action = Some(UnsavedAction::CancelEdit);
            self.set_message("unsaved changes · s save · d discard · Esc continue", at);
        } else {
            self.cancel_editor(at);
        }
    }

    fn request_quit(&mut self, at: Instant) {
        if self.editor_dirty() {
            self.unsaved_action = Some(UnsavedAction::Quit);
            self.set_message("unsaved changes · s save · d discard · Esc continue", at);
        } else {
            self.quit = true;
        }
    }

    fn finish_editor_action(&mut self, action: UnsavedAction, at: Instant) {
        match action {
            UnsavedAction::CancelEdit => self.cancel_editor(at),
            UnsavedAction::Quit => self.quit = true,
        }
    }

    fn discard_editor(&mut self, at: Instant, action: UnsavedAction) {
        self.editor = None;
        self.editor_scroll = 0;
        self.editor_goal_column = None;
        self.load_active_file();
        match action {
            UnsavedAction::CancelEdit => self.set_message("changes discarded", at),
            UnsavedAction::Quit => self.quit = true,
        }
    }

    fn editor_move(&mut self, action: impl FnOnce(&mut EditorBuffer)) {
        if let Some(editor) = self.editor.as_mut() {
            action(editor);
        }
    }

    /// Brings the cursor's visual row on screen, and keeps the viewport from
    /// scrolling past the end of a wrapped document.
    fn ensure_editor_cursor_visible(&mut self) {
        let Some(editor) = self.editor.as_ref() else {
            self.editor_scroll = 0;
            return;
        };

        let (row, _) = self.editor_wrap.locate(editor.cursor(), editor.lines());
        let height = self.document_height().max(1);
        let last_top = self.editor_wrap.len().saturating_sub(height);

        if row < self.editor_scroll {
            self.editor_scroll = row;
        } else if row >= self.editor_scroll.saturating_add(height) {
            self.editor_scroll = row.saturating_sub(height.saturating_sub(1));
        }
        self.editor_scroll = self.editor_scroll.min(last_top);
    }

    fn enter_editor(&mut self, at: Instant) {
        let Some(path) = self.workspace.active_path() else {
            self.set_message("stdin content is read-only", at);
            return;
        };
        let path_display = path.display().to_string();
        match self.workspace.reload_content() {
            Ok(content) => {
                self.editor = Some(EditorBuffer::from_text(&content));
                self.editor_scroll = 0;
                self.editor_goal_column = None;
                self.last_modified = modified_time(self.workspace.active_path());
                self.external_change_detected = false;
                self.clear_selection();
                self.clear_search();
                self.focus = Focus::Document;
                self.set_message(format!("editing {path_display} · ^S save · Esc leave"), at);
            }
            Err(error) => {
                let message = format!("Cannot edit file: {error}");
                self.error = Some(message.clone());
                self.set_message(message, at);
            }
        }
    }

    fn save_editor(&mut self, at: Instant) -> bool {
        let Some(content) = self.editor.as_ref().map(EditorBuffer::text) else {
            return false;
        };
        match self.workspace.save_content(&content) {
            Ok(()) => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.mark_clean();
                }
                self.apply_document_content(&content);
                self.external_change_detected = false;
                self.set_message("saved", at);
                true
            }
            Err(error) => {
                let message = format!("Cannot save file: {error}");
                self.error = Some(message.clone());
                self.set_message(message, at);
                false
            }
        }
    }

    fn cancel_editor(&mut self, at: Instant) {
        self.editor = None;
        self.editor_scroll = 0;
        self.editor_goal_column = None;
        self.load_active_file();
        self.set_message("edit cancelled", at);
    }

    fn reload_editor_from_disk(&mut self) -> bool {
        match self.workspace.reload_content() {
            Ok(content) => {
                self.editor = Some(EditorBuffer::from_text(&content));
                self.editor_scroll = 0;
                self.editor_goal_column = None;
                self.apply_document_content(&content);
                self.external_change_detected = false;
                true
            }
            Err(error) => {
                self.error = Some(format!("Cannot reload file: {error}"));
                false
            }
        }
    }

    /// Returns whether the event changed anything worth redrawing.
    fn handle_mouse(&mut self, event: crossterm::event::MouseEvent) -> bool {
        use crossterm::event::{MouseButton, MouseEventKind};

        if self.focus != Focus::Document
            || self.unsaved_action.is_some()
            || self.external_prompt.is_some()
        {
            return false;
        }

        match event.kind {
            MouseEventKind::ScrollUp if self.mouse_is_over_reader(event.column, event.row) => {
                self.aim_scroll(-MOUSE_WHEEL_LINES)
            }
            MouseEventKind::ScrollDown if self.mouse_is_over_reader(event.column, event.row) => {
                self.aim_scroll(MOUSE_WHEEL_LINES)
            }
            MouseEventKind::Down(MouseButton::Left) if self.is_editing() => {
                match self.editor_position_at(event.column, event.row) {
                    Some(position) => {
                        self.set_editor_cursor(position);
                        true
                    }
                    None => false,
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                match self.document_position_at(event.column, event.row) {
                    Some(position) => {
                        self.start_selection(position);
                        true
                    }
                    None => false,
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if !self.is_editing() => {
                match self.document_position_at(event.column, event.row) {
                    Some(position) => {
                        self.update_selection(position);
                        true
                    }
                    None => false,
                }
            }
            MouseEventKind::Up(MouseButton::Left)
                if !self.is_editing()
                    && self
                        .selection
                        .as_ref()
                        .is_some_and(|selection| selection.is_empty()) =>
            {
                self.clear_selection();
                true
            }
            _ => false,
        }
    }

    /// Adds `lines` to the distance the viewport still has to travel. Returns
    /// whether there was anywhere left to travel to.
    fn aim_scroll(&mut self, lines: f32) -> bool {
        let (position, limit) = if self.is_editing() {
            (self.editor_scroll, self.editor_max_scroll())
        } else {
            (self.scroll, self.max_scroll())
        };
        let room = if lines < 0.0 {
            position > 0
        } else {
            position < limit
        };
        if !room {
            self.end_glide();
            return false;
        }
        self.scroll_debt += lines;
        true
    }

    fn gliding(&self) -> bool {
        self.scroll_debt.abs() >= f32::EPSILON
    }

    fn end_glide(&mut self) {
        self.scroll_debt = 0.0;
        self.scroll_advanced = None;
    }

    /// Pays off part of the outstanding scroll distance, by elapsed time rather
    /// than by frame count, so the motion keeps its pace whatever the frame
    /// rate is doing and spreads evenly between notches.
    fn advance_scroll(&mut self, at: Instant) -> bool {
        if !self.gliding() {
            return false;
        }
        let elapsed = match self.scroll_advanced {
            Some(last) => at.saturating_duration_since(last).min(MAX_FRAME_GAP),
            None => Duration::from_millis(16),
        };
        self.scroll_advanced = Some(at);

        let owed = self.scroll_debt.abs();
        let direction = self.scroll_debt.signum();
        let rate = (owed * SCROLL_RATE_GAIN).clamp(SCROLL_MIN_RATE, SCROLL_MAX_RATE);
        let remaining = (owed - rate * elapsed.as_secs_f32()).max(0.0);

        // Whole lines still owed before and after: the difference is exactly
        // what to move now, so no fraction is ever rounded away or double paid.
        let lines = (owed.ceil() - remaining.ceil()) as usize;
        self.scroll_debt = if remaining < f32::EPSILON {
            0.0
        } else {
            remaining * direction
        };

        let before = (self.scroll, self.editor_scroll);
        for _ in 0..lines {
            match (direction > 0.0, self.is_editing()) {
                (true, true) => self.editor_scroll_down(),
                (true, false) => self.move_down(),
                (false, true) => self.editor_scroll_up(),
                (false, false) => self.move_up(),
            }
        }

        if lines > 0 && before == (self.scroll, self.editor_scroll) {
            // Against a stop: there is nowhere left to glide to.
            self.end_glide();
            return false;
        }
        true
    }

    fn editor_max_scroll(&self) -> usize {
        self.editor_lines()
            .len()
            .saturating_sub(self.document_height())
    }

    /// Moves the viewport outright, abandoning any glide still in flight.
    fn jump_scroll(&mut self, line: usize) {
        self.scroll = line.min(self.max_scroll());
        self.end_glide();
    }

    fn set_editor_cursor(&mut self, position: CursorPosition) {
        self.editor_goal_column = None;
        if let Some(editor) = self.editor.as_mut() {
            editor.set_cursor(position);
            self.ensure_editor_cursor_visible();
        }
    }

    fn editor_position_at(&self, screen_x: u16, screen_y: u16) -> Option<CursorPosition> {
        if !self.mouse_is_over_reader(screen_x, screen_y) {
            return None;
        }

        let document_area = self.reader_outer_area();
        let inner = layout::editor_inner(document_area);
        let (inner_x, inner_y) = (inner.x, inner.y);
        let row = self
            .editor_scroll
            .saturating_add(usize::from(screen_y.saturating_sub(inner_y)));
        if row >= self.editor_wrap.len() {
            return None;
        }

        let text_start = inner_x.saturating_add(self.editor_gutter_width() as u16);
        let display_column = usize::from(screen_x.saturating_sub(text_start));
        let editor = self.editor.as_ref()?;
        Some(
            self.editor_wrap
                .position_at(row, display_column, editor.lines()),
        )
    }

    fn mouse_is_over_reader(&self, screen_x: u16, screen_y: u16) -> bool {
        if self.responsive_mode == ResponsiveMode::Overlay
            && self.sidebar_visible
            && screen_x < self.sidebar_width()
        {
            return false;
        }

        let document_area = self.reader_outer_area();
        let inner = layout::reader_inner(document_area);
        let (inner_x, inner_y) = (inner.x, inner.y);
        let (inner_width, inner_height) = (inner.width, inner.height);

        screen_x >= inner_x
            && screen_x < inner_x.saturating_add(inner_width)
            && screen_y >= inner_y
            && screen_y < inner_y.saturating_add(inner_height)
    }

    fn document_position_at(&self, screen_x: u16, screen_y: u16) -> Option<CursorPosition> {
        if self.responsive_mode == ResponsiveMode::Overlay
            && self.sidebar_visible
            && screen_x < self.sidebar_width()
        {
            return None;
        }

        let document_area = self.reader_outer_area();
        if document_area.width == 0 || document_area.height == 0 {
            return None;
        }

        let inner = layout::reader_inner(document_area);
        let (inner_x, inner_y) = (inner.x, inner.y);
        let (inner_width, inner_height) = (inner.width, inner.height);

        if screen_x < inner_x
            || screen_x >= inner_x.saturating_add(inner_width)
            || screen_y < inner_y
            || screen_y >= inner_y + inner_height
        {
            return None;
        }

        let relative_x = screen_x - inner_x;
        let relative_y = screen_y - inner_y;
        let line = self.scroll.saturating_add(usize::from(relative_y));
        if line >= self.document_layout.lines.len() {
            return None;
        }

        let margin = self.document_layout.content_margin;
        let column = if usize::from(relative_x) > margin {
            usize::from(relative_x).saturating_sub(margin)
        } else {
            0
        };

        Some(CursorPosition::new(line, column))
    }

    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_search(),
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

    fn cancel_search(&mut self) {
        self.search_input = None;
        self.search_query.clear();
        self.search_matches.clear();
        self.search_selected = 0;
    }

    pub fn start_selection(&mut self, position: CursorPosition) {
        let Some(position) = crate::selection::clamp_position_with_margin(
            position,
            &self.document_layout.lines,
            self.document_layout.content_margin,
        ) else {
            self.clear_selection();
            return;
        };
        self.select_mode = true;
        self.selection = Some(Selection::new(position, position));
    }

    pub fn update_selection(&mut self, position: CursorPosition) {
        if let Some(position) = crate::selection::clamp_position_with_margin(
            position,
            &self.document_layout.lines,
            self.document_layout.content_margin,
        ) && let Some(selection) = &mut self.selection
        {
            selection.head = position;
        }
    }

    pub fn clear_selection(&mut self) {
        self.select_mode = false;
        self.selection = None;
    }

    fn clamp_selection(&mut self) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let lines = &self.document_layout.lines;
        let anchor = crate::selection::clamp_position_with_margin(
            selection.anchor,
            lines,
            self.document_layout.content_margin,
        );
        let head = crate::selection::clamp_position_with_margin(
            selection.head,
            lines,
            self.document_layout.content_margin,
        );
        match (anchor, head) {
            (Some(anchor), Some(head)) => {
                if let Some(selection) = self.selection.as_mut() {
                    selection.anchor = anchor;
                    selection.head = head;
                }
            }
            _ => self.clear_selection(),
        }
    }

    pub fn copy_selection(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(selection) = &self.selection else {
            return Ok(());
        };
        let text = selection.text_with_margin(
            &self.document_layout.lines,
            self.document_layout.content_margin,
        );
        if text.is_empty() {
            return Ok(());
        }
        let mut clipboard = arboard::Clipboard::new()?;
        clipboard.set_text(text)?;
        Ok(())
    }

    fn selection_move_up(&mut self) {
        let Some(selection) = &mut self.selection else {
            return;
        };
        let new_line = selection.head.line.saturating_sub(1);
        selection.head = CursorPosition::new(new_line, selection.head.column);
        self.clamp_selection();
    }

    fn selection_move_down(&mut self) {
        let Some(selection) = &mut self.selection else {
            return;
        };
        let new_line = selection
            .head
            .line
            .saturating_add(1)
            .min(self.document_layout.lines.len().saturating_sub(1));
        selection.head = CursorPosition::new(new_line, selection.head.column);
        self.clamp_selection();
    }

    fn selection_move_left(&mut self) {
        let Some(selection) = &mut self.selection else {
            return;
        };
        let column = selection.head.column.saturating_sub(1);
        selection.head = CursorPosition::new(selection.head.line, column);
    }

    fn selection_move_right(&mut self) {
        let Some(selection) = &mut self.selection else {
            return;
        };
        let Some(line) = self.document_layout.lines.get(selection.head.line) else {
            return;
        };
        let text = crate::selection::content_text(line, self.document_layout.content_margin);
        let column = selection
            .head
            .column
            .saturating_add(1)
            .min(crate::selection::text_width(&text));
        selection.head = CursorPosition::new(selection.head.line, column);
    }

    fn confirm_search(&mut self) {
        self.search_query = self.search_input.take().unwrap_or_default();
        self.refresh_search();
        self.search_selected = 0;
        if let Some(search_match) = self.search_matches.first() {
            self.jump_scroll(search_match.line);
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
        self.jump_scroll(self.search_matches[self.search_selected].line);
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
        self.jump_scroll(self.search_matches[self.search_selected].line);
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

    fn move_up(&mut self) {
        match self.focus {
            Focus::Sidebar => match self.sidebar_panel {
                SidebarPanel::Outline => {
                    self.outline_selected = self.outline_selected.saturating_sub(1)
                }
                SidebarPanel::Files => self.file_explorer.move_up(),
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
                    self.file_explorer.move_down();
                }
            },
            // Clamped here rather than at the call sites: an unclamped counter
            // runs away past the end of the document and then needs as many
            // presses to come back.
            Focus::Document => self.scroll = self.scroll.saturating_add(1).min(self.max_scroll()),
        }
    }

    fn editor_scroll_up(&mut self) {
        self.editor_scroll = self.editor_scroll.saturating_sub(1);
    }

    fn editor_scroll_down(&mut self) {
        // Rows, not lines: a wrapped document is taller than its line count.
        let max_scroll = self
            .editor_wrap
            .len()
            .saturating_sub(self.document_height());
        self.editor_scroll = self.editor_scroll.saturating_add(1).min(max_scroll);
    }

    fn page_up(&mut self) {
        let amount = self.terminal_height.saturating_sub(5) as usize;
        self.jump_scroll(self.scroll.saturating_sub(amount.max(1)));
    }

    fn page_down(&mut self) {
        let amount = self.terminal_height.saturating_sub(5) as usize;
        self.jump_scroll(self.scroll.saturating_add(amount.max(1)));
    }

    fn jump_to_selected_heading(&mut self) {
        if self.document.outline.get(self.outline_selected).is_some() {
            if let Some(line) = self.document_layout.heading_line(self.outline_selected) {
                self.jump_scroll(line);
            }
            self.focus = Focus::Document;
        }
    }

    fn select_sidebar_panel(&mut self, panel: SidebarPanel, at: Instant) {
        self.clear_selection();
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
                self.jump_scroll(0);
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
        self.jump_scroll(0);
        self.clear_search();
        self.load_active_file();
    }

    fn reload_if_changed(&mut self) -> bool {
        if self.is_editing() {
            if self.workspace.active_path().is_some()
                && modified_time(self.workspace.active_path()) != self.last_modified
                && !self.external_change_detected
            {
                self.external_change_detected = true;
                self.set_message(
                    "file changed on disk · review before saving",
                    Instant::now(),
                );
                return true;
            }
            return false;
        }
        let current_modified = modified_time(self.workspace.active_path());
        if current_modified.is_some() && current_modified != self.last_modified {
            self.load_active_file();
            true
        } else {
            false
        }
    }

    fn load_active_file(&mut self) {
        self.editor = None;
        self.editor_scroll = 0;
        self.editor_goal_column = None;
        self.external_change_detected = false;
        self.external_prompt = None;
        match self.workspace.reload_content() {
            Ok(content) => self.apply_document_content(&content),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn apply_document_content(&mut self, content: &str) {
        self.document = Document::parse(content);
        self.outline_selected = self
            .outline_selected
            .min(self.document.outline.len().saturating_sub(1));
        self.last_modified = modified_time(self.workspace.active_path());
        self.error = None;
        self.reload_images();
        self.rebuild_layout();
        self.clear_selection();
        self.clear_search();
        self.clamp_scroll();
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
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Instant, SystemTime};

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
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

    fn temporary_document() -> (PathBuf, App) {
        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("markr-editor-{}-{id}.md", std::process::id()));
        fs::write(&path, "# One").expect("editor fixture");
        let workspace = Workspace::open(Some(path.clone()), true).expect("workspace");
        let app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        (path, app)
    }

    fn key(code: KeyCode) -> Message {
        Message::Key {
            key: KeyEvent::new(code, KeyModifiers::NONE),
            at: Instant::now(),
        }
    }

    fn control_key(code: KeyCode) -> Message {
        Message::Key {
            key: KeyEvent::new(code, KeyModifiers::CONTROL),
            at: Instant::now(),
        }
    }

    const FRAME: std::time::Duration = std::time::Duration::from_millis(16);

    /// Runs frames until the wheel glide has finished, as the event loop does.
    /// The clock is advanced explicitly because the glide travels by elapsed
    /// time rather than by frame count.
    fn drain_scroll(app: &mut App, clock: &mut Instant) -> usize {
        for frame in 0..400 {
            *clock += FRAME;
            app.update(Message::Frame { at: *clock });
            if !app.gliding() {
                return frame + 1;
            }
        }
        panic!("the glide never finished");
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Message {
        Message::Mouse {
            event: MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
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
    fn edits_saves_and_rebuilds_the_active_document() {
        let (path, mut app) = temporary_document();

        app.update(key(KeyCode::Char('e')));
        assert!(app.is_editing());
        app.update(key(KeyCode::End));
        app.update(key(KeyCode::Char('!')));
        assert!(app.editor_dirty());

        app.update(control_key(KeyCode::Char('s')));

        assert_eq!(fs::read_to_string(&path).expect("saved document"), "# One!");
        assert!(!app.editor_dirty());
        assert_eq!(app.document.outline[0].title, "One!");

        app.update(key(KeyCode::Esc));
        assert!(!app.is_editing());
        fs::remove_file(path).expect("remove editor fixture");
    }

    #[test]
    fn cancelling_edits_leaves_the_file_unchanged() {
        let (path, mut app) = temporary_document();

        app.update(key(KeyCode::Char('e')));
        app.update(key(KeyCode::End));
        app.update(key(KeyCode::Char('!')));
        app.update(key(KeyCode::Esc));
        app.update(key(KeyCode::Char('d')));

        assert!(!app.is_editing());
        assert_eq!(
            fs::read_to_string(&path).expect("original document"),
            "# One"
        );
        fs::remove_file(path).expect("remove editor fixture");
    }

    #[test]
    fn prompts_before_cancelling_dirty_edits() {
        let (path, mut app) = temporary_document();

        app.update(key(KeyCode::Char('e')));
        app.update(key(KeyCode::End));
        app.update(key(KeyCode::Char('!')));
        app.update(key(KeyCode::Esc));

        assert!(app.has_unsaved_prompt());
        assert!(app.is_editing());

        app.update(key(KeyCode::Esc));
        assert!(!app.has_unsaved_prompt());
        assert!(app.is_editing());
        assert!(app.editor_dirty());

        app.update(key(KeyCode::Esc));
        assert!(app.has_unsaved_prompt());
        app.update(key(KeyCode::Char('d')));
        assert!(!app.is_editing());
        assert_eq!(
            fs::read_to_string(&path).expect("unchanged document"),
            "# One"
        );
        fs::remove_file(path).expect("remove editor fixture");
    }

    #[test]
    fn prompts_before_quitting_and_can_save_first() {
        let (path, mut app) = temporary_document();

        app.update(key(KeyCode::Char('e')));
        app.update(key(KeyCode::End));
        app.update(key(KeyCode::Char('!')));
        app.update(control_key(KeyCode::Char('c')));

        assert!(app.has_unsaved_prompt());
        assert!(!app.should_quit());

        app.update(key(KeyCode::Char('s')));
        assert!(app.should_quit());
        assert_eq!(fs::read_to_string(&path).expect("saved document"), "# One!");
        fs::remove_file(path).expect("remove editor fixture");
    }

    #[test]
    fn mouse_click_places_the_editor_cursor() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        app.update(key(KeyCode::Char('e')));

        let reader = app.reader_outer_area();
        let inner = crate::layout::editor_inner(reader);
        let (inner_x, inner_y) = (inner.x, inner.y);
        let line_number_width = app.editor_lines().len().to_string().len();
        let text_start = inner_x.saturating_add(line_number_width as u16 + 5);
        app.update(mouse(
            MouseEventKind::Down(MouseButton::Left),
            text_start + 2,
            inner_y,
        ));

        assert_eq!(
            app.editor_cursor(),
            Some(crate::selection::CursorPosition::new(0, 2))
        );
    }

    #[test]
    fn editor_undo_and_redo_update_the_buffer() {
        let (path, mut app) = temporary_document();

        app.update(key(KeyCode::Char('e')));
        app.update(key(KeyCode::End));
        app.update(key(KeyCode::Char('!')));
        app.update(control_key(KeyCode::Char('z')));
        assert_eq!(app.editor_text().as_deref(), Some("# One"));
        assert!(!app.editor_dirty());

        app.update(control_key(KeyCode::Char('y')));
        assert_eq!(app.editor_text().as_deref(), Some("# One!"));
        assert!(app.editor_dirty());
        fs::remove_file(path).expect("remove editor fixture");
    }

    /// An editor open on one paragraph long enough to wrap several times,
    /// followed by a line with a single character to fall onto.
    fn wrapped_document() -> (PathBuf, App) {
        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("markr-wrap-{}-{id}.md", std::process::id()));
        let long = ["alpha"; 40].join(" ");
        fs::write(&path, format!("{long}\ne")).expect("wrap fixture");
        let workspace = Workspace::open(Some(path.clone()), true).expect("workspace");
        let mut app = App::new(workspace, Picker::halfblocks(), Theme::default()).expect("app");
        app.update(Message::Resize {
            width: 120,
            height: 20,
            at: Instant::now(),
        });
        app.update(key(KeyCode::Char('e')));
        (path, app)
    }

    /// The cursor's visual row, and how far across it sits in cells.
    fn cursor_cell(app: &App) -> (usize, usize) {
        app.editor_wrap()
            .locate(app.editor_cursor().expect("cursor"), app.editor_lines())
    }

    #[test]
    fn a_long_line_takes_several_rows_instead_of_scrolling_sideways() {
        let (path, app) = wrapped_document();

        assert_eq!(app.editor_lines().len(), 2, "two source lines");
        assert!(
            app.editor_wrap().len() > 3,
            "but several rows once wrapped: {}",
            app.editor_wrap().len()
        );

        fs::remove_file(path).expect("remove wrap fixture");
    }

    #[test]
    fn down_travels_one_visual_row_not_one_source_line() {
        let (path, mut app) = wrapped_document();
        assert_eq!(cursor_cell(&app), (0, 0));

        app.update(key(KeyCode::Down));

        assert_eq!(cursor_cell(&app).0, 1, "one row down");
        assert_eq!(
            app.editor_cursor().expect("cursor").line,
            0,
            "still inside the first source line, which is the whole point"
        );

        fs::remove_file(path).expect("remove wrap fixture");
    }

    #[test]
    fn the_cursor_keeps_the_cell_it_was_aiming_for_across_a_short_row() {
        let (path, mut app) = wrapped_document();

        for _ in 0..5 {
            app.update(key(KeyCode::Right));
        }
        let aim = cursor_cell(&app).1;
        assert_eq!(aim, 5);

        // Fall onto the one-character last line. It cannot hold the aim.
        while app.editor_cursor().expect("cursor").line == 0 {
            app.update(key(KeyCode::Down));
        }
        assert_eq!(cursor_cell(&app).1, 1, "clamped to the short line's end");

        // Coming back up, the aim is restored rather than lost to that line.
        app.update(key(KeyCode::Up));

        assert_eq!(
            cursor_cell(&app).1,
            aim,
            "the cell the cursor was aiming for survived the short line"
        );

        fs::remove_file(path).expect("remove wrap fixture");
    }

    #[test]
    fn a_sideways_move_gives_up_the_aim() {
        let (path, mut app) = wrapped_document();

        for _ in 0..5 {
            app.update(key(KeyCode::Right));
        }
        while app.editor_cursor().expect("cursor").line == 0 {
            app.update(key(KeyCode::Down));
        }
        // Moving by hand on the short line replaces the aim with where the
        // cursor actually is, so going back up does not jump to cell five.
        app.update(key(KeyCode::Left));
        app.update(key(KeyCode::Up));

        assert_eq!(cursor_cell(&app).1, 0);

        fs::remove_file(path).expect("remove wrap fixture");
    }

    #[test]
    fn home_and_end_work_on_the_visual_row() {
        let (path, mut app) = wrapped_document();

        app.update(key(KeyCode::Down));
        assert_eq!(cursor_cell(&app), (1, 0));

        app.update(key(KeyCode::End));
        let (row, cell) = cursor_cell(&app);
        assert_eq!(row, 1, "End stayed on the row it started on");
        assert!(cell > 0);
        assert!(
            app.editor_cursor().expect("cursor").column < app.editor_lines()[0].chars().count(),
            "End stopped at the end of the row, not the end of the source line"
        );

        app.update(key(KeyCode::Home));

        assert_eq!(
            cursor_cell(&app),
            (1, 0),
            "Home returned to the row's start"
        );

        fs::remove_file(path).expect("remove wrap fixture");
    }

    #[test]
    fn the_wheel_scrolls_the_editor_without_dragging_the_cursor_along() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        app.update(key(KeyCode::Char('e')));
        let cursor = app.editor_cursor().expect("cursor");

        let mut clock = Instant::now();
        app.update(mouse(MouseEventKind::ScrollDown, 35, 3));
        drain_scroll(&mut app, &mut clock);

        assert!(app.editor_scroll > 0, "the viewport moved");
        assert_eq!(
            app.editor_cursor().expect("cursor"),
            cursor,
            "but looking around left the caret where it was"
        );
    }

    #[test]
    fn every_letter_can_be_typed_into_the_editor() {
        let (path, mut app) = temporary_document();
        app.update(key(KeyCode::Char('e')));

        // `q` used to quit from inside the editor, so it could not be
        // written — and neither could anything after it, because the unsaved
        // prompt swallowed the next key.
        for character in "quick brown".chars() {
            app.update(key(KeyCode::Char(character)));
        }

        assert_eq!(app.editor_text().as_deref(), Some("quick brown# One"));
        assert!(!app.should_quit());
        assert!(!app.has_unsaved_prompt());

        fs::remove_file(path).expect("remove editor fixture");
    }

    #[test]
    fn editor_highlighting_follows_the_buffer_and_the_palette() {
        let (path, mut app) = temporary_document();

        // Nothing to highlight until the editor is open.
        assert!(app.editor_highlight().is_empty());

        app.update(key(KeyCode::Char('e')));
        assert_eq!(app.editor_highlight().len(), app.editor_lines().len());

        // A typed newline adds a line, and the highlighting keeps pace so an
        // index into it stays an index into the buffer.
        app.update(key(KeyCode::End));
        app.update(key(KeyCode::Enter));
        app.update(key(KeyCode::Char('x')));
        assert_eq!(app.editor_highlight().len(), app.editor_lines().len());

        // Repainting the walls re-colours the source underneath.
        let before = app.editor_highlight().to_vec();
        app.update(key(KeyCode::Char('T')));
        assert_ne!(app.editor_highlight(), before.as_slice());

        // Leaving the editor drops the held lines.
        app.update(control_key(KeyCode::Char('s')));
        app.update(key(KeyCode::Esc));
        assert!(app.editor_highlight().is_empty());

        fs::remove_file(path).expect("remove editor fixture");
    }

    /// The text the held highlighting would actually draw.
    fn highlighted_text(app: &App) -> String {
        app.editor_highlight()
            .iter()
            .map(|line| {
                line.iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn reloading_an_untouched_buffer_refreshes_the_highlighting() {
        let (path, mut app) = temporary_document();

        // The editor draws from the highlighting rather than from the buffer,
        // so highlighting left over from the replaced file would keep the old
        // text on screen — the one thing reloading is supposed to fix. This
        // buffer is never edited, which is the case a per-buffer revision
        // counter would miss.
        app.update(key(KeyCode::Char('e')));
        assert_eq!(highlighted_text(&app), "# One");

        app.last_modified = Some(SystemTime::UNIX_EPOCH);
        fs::write(&path, "# External").expect("external document change");
        app.update(Message::Tick);
        app.update(control_key(KeyCode::Char('s')));
        app.update(key(KeyCode::Char('r')));

        assert_eq!(app.editor_text().as_deref(), Some("# External"));
        assert_eq!(highlighted_text(&app), "# External");

        fs::remove_file(path).expect("remove editor fixture");
    }

    #[test]
    fn detects_external_changes_and_can_reload_them() {
        let (path, mut app) = temporary_document();

        app.update(key(KeyCode::Char('e')));
        app.last_modified = Some(SystemTime::UNIX_EPOCH);
        fs::write(&path, "# External").expect("external document change");
        app.update(Message::Tick);

        assert!(app.external_change_detected);
        assert_eq!(app.editor_text().as_deref(), Some("# One"));

        app.update(control_key(KeyCode::Char('s')));
        assert!(app.has_external_prompt());
        app.update(key(KeyCode::Char('r')));

        assert!(!app.has_external_prompt());
        assert!(!app.external_change_detected);
        assert_eq!(app.editor_text().as_deref(), Some("# External"));
        fs::remove_file(path).expect("remove editor fixture");
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
    fn escape_cancels_active_search() {
        let mut app = readme_app();

        app.update(key(KeyCode::Char('/')));
        assert!(app.search_input.is_some());

        app.update(key(KeyCode::Char('m')));
        app.update(key(KeyCode::Char('a')));
        app.update(key(KeyCode::Char('r')));
        app.update(key(KeyCode::Enter));
        assert!(!app.search_query.is_empty());
        assert!(!app.search_matches.is_empty());

        app.update(key(KeyCode::Esc));
        assert!(app.search_input.is_none());
        assert!(app.search_query.is_empty());
        assert!(app.search_matches.is_empty());
    }

    #[test]
    fn keyboard_selection_requests_redraw_and_stays_in_document_bounds() {
        let mut app = readme_app();

        assert!(app.update(key(KeyCode::Char('v'))));
        assert!(app.select_mode);
        assert!(app.selection.is_some());

        for _ in 0..10_000 {
            app.update(key(KeyCode::Right));
        }
        let selection = app.selection.as_ref().expect("active selection");
        let line = &app.document_layout.lines[selection.head.line];
        let text = crate::selection::line_text(line);
        assert!(selection.head.column <= crate::selection::text_width(&text));

        assert!(app.update(key(KeyCode::Esc)));
        assert!(!app.select_mode);
        assert!(app.selection.is_none());
    }

    #[test]
    fn invalid_selection_positions_are_clamped_to_document_bounds() {
        let mut app = readme_app();

        app.start_selection(crate::selection::CursorPosition::new(
            usize::MAX,
            usize::MAX,
        ));

        assert!(app.select_mode);
        assert!(app.selection.is_some());
        let selection = app.selection.as_ref().expect("clamped selection");
        assert!(selection.anchor.line < app.document_layout.lines.len());
    }

    #[test]
    fn mouse_wheel_scrolls_the_reader_but_not_the_sidebar() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });

        let mut clock = Instant::now();
        app.update(mouse(MouseEventKind::ScrollDown, 35, 3));
        drain_scroll(&mut app, &mut clock);
        assert_eq!(app.scroll, super::MOUSE_WHEEL_LINES as usize);

        app.update(mouse(MouseEventKind::ScrollUp, 35, 3));
        drain_scroll(&mut app, &mut clock);
        assert_eq!(app.scroll, 0);

        app.update(key(KeyCode::Tab));
        app.update(mouse(MouseEventKind::ScrollDown, 5, 3));
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn a_wheel_notch_arrives_a_line_at_a_time_rather_than_at_once() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });

        app.update(mouse(MouseEventKind::ScrollDown, 60, 5));
        assert_eq!(app.scroll, 0, "the notch should not land on the event");

        let mut clock = Instant::now();
        let mut seen = Vec::new();
        for _ in 0..12 {
            clock += FRAME;
            app.update(Message::Frame { at: clock });
            seen.push(app.scroll);
        }

        // Never more than a line in any one frame: three lines fired off in a
        // single frame is what read as a jump next to a trackpad.
        assert!(
            seen.windows(2).all(|pair| pair[1] - pair[0] <= 1),
            "the notch jumped: {seen:?}"
        );
        // And spread out rather than crammed into the first frames.
        assert!(
            seen[0] == 0,
            "the notch started landing immediately: {seen:?}"
        );
        assert_eq!(app.scroll, super::MOUSE_WHEEL_LINES as usize);
    }

    #[test]
    fn a_burst_of_notches_is_caught_up_with_rather_than_queued() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });

        let notches = 20;
        for _ in 0..notches {
            app.update(mouse(MouseEventKind::ScrollDown, 60, 5));
        }

        // The rate rises with the distance outstanding, so a flick is caught up
        // with rather than played back one notch at a time. At the single-notch
        // rate sixty lines would take well over a hundred frames.
        let mut clock = Instant::now();
        let frames = drain_scroll(&mut app, &mut clock);
        assert!(frames < 60, "a burst queued up: {frames} frames");
        assert_eq!(app.scroll, notches * super::MOUSE_WHEEL_LINES as usize);
    }

    #[test]
    fn scrolling_the_reader_starts_no_animation() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });

        // Past the start-up animation window, so only a transition started by
        // the scroll itself could keep the clock running.
        let at = Instant::now() + std::time::Duration::from_millis(300);
        assert!(!app.animation_active(at));

        app.update(Message::Mouse {
            event: MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 60,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
        });
        let mut clock = at;
        drain_scroll(&mut app, &mut clock);
        assert_eq!(app.scroll, super::MOUSE_WHEEL_LINES as usize);

        // Scrolling used to restart a transition that drove nothing visible,
        // which pinned the event loop to a sixteen millisecond poll — a full
        // screen repaint every frame for ninety milliseconds after every notch.
        assert!(!app.animation_active(at + std::time::Duration::from_millis(10)));
    }

    #[test]
    fn only_redraws_for_mouse_events_that_change_something() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });

        // Mouse capture delivers motion events; they change nothing.
        assert!(!app.update(mouse(MouseEventKind::Moved, 60, 5)));
        // A scroll that has somewhere to go does.
        assert!(app.update(mouse(MouseEventKind::ScrollDown, 60, 5)));
        let mut clock = Instant::now();
        drain_scroll(&mut app, &mut clock);
        // A scroll already against the top does not.
        app.jump_scroll(0);
        assert!(!app.update(mouse(MouseEventKind::ScrollUp, 60, 5)));
    }

    #[test]
    fn mouse_wheel_scrolls_the_editor_viewport() {
        let mut app = readme_app();
        app.update(Message::Resize {
            width: 120,
            height: 40,
            at: Instant::now(),
        });
        app.update(key(KeyCode::Char('e')));
        assert!(app.editor_lines().len() > app.document_height());

        let mut clock = Instant::now();
        app.update(mouse(MouseEventKind::ScrollDown, 35, 3));
        drain_scroll(&mut app, &mut clock);
        assert_eq!(app.editor_scroll, super::MOUSE_WHEEL_LINES as usize);
        assert_eq!(app.scroll, 0);

        app.update(mouse(MouseEventKind::ScrollUp, 35, 3));
        drain_scroll(&mut app, &mut clock);
        assert_eq!(app.editor_scroll, 0);
        assert_eq!(app.scroll, 0);
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
