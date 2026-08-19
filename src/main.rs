mod app;
mod event;
mod explorer;
mod images;
mod layout;
mod markdown;
mod syntax;
mod theme;
mod view;
mod workspace;

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use crossterm::cursor::Show;
use crossterm::event::{
    self as terminal_event, Event as CrosstermEvent, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui_image::picker::Picker;

use crate::app::{App, Message};
use crate::event::map_event;
use crate::workspace::Workspace;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(150);
const MAX_EVENTS_PER_FRAME: usize = 64;

#[derive(Debug, Parser)]
#[command(
    name = "markr",
    version,
    about = "A polished Markdown workspace for the terminal"
)]
struct Cli {
    /// Markdown file or directory to open. If omitted, stdin is used when piped.
    path: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let stdin_is_terminal = io::stdin().is_terminal();
    let workspace = Workspace::open(cli.path, stdin_is_terminal)?;

    enable_raw_mode()?;
    let mut terminal_session = TerminalSession {
        alternate_screen: false,
        keyboard_enhancement: false,
    };
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    terminal_session.alternate_screen = true;
    if supports_keyboard_enhancement().unwrap_or(false) {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )?;
        terminal_session.keyboard_enhancement = true;
    }
    let picker = if stdin_is_terminal {
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
    } else {
        Picker::halfblocks()
    };
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let size = terminal.size()?;
    let mut app = App::new(workspace, picker)?;
    app.update(Message::Resize {
        width: size.width,
        height: size.height,
    });

    run(&mut terminal, &mut app)
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|frame| view::render(frame, app))?;

    while !app.should_quit() {
        let mut redraw = false;
        if terminal_event::poll(EVENT_POLL_INTERVAL)? {
            for _ in 0..MAX_EVENTS_PER_FRAME {
                redraw |= handle_terminal_event(app, terminal_event::read()?);
                if app.should_quit() || !terminal_event::poll(Duration::ZERO)? {
                    break;
                }
            }
        } else {
            redraw = app.update(Message::Tick);
        }

        if redraw && !app.should_quit() {
            terminal.draw(|frame| view::render(frame, app))?;
        }
    }

    Ok(())
}

fn handle_terminal_event(app: &mut App, event: CrosstermEvent) -> bool {
    match event {
        CrosstermEvent::Resize(width, height) => app.update(Message::Resize { width, height }),
        event => map_event(event).is_some_and(|message| app.update(message)),
    }
}

struct TerminalSession {
    alternate_screen: bool,
    keyboard_enhancement: bool,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.keyboard_enhancement {
            let _ = execute!(stdout, PopKeyboardEnhancementFlags);
        }
        let _ = disable_raw_mode();
        if self.alternate_screen {
            let _ = execute!(stdout, LeaveAlternateScreen, Show);
        }
    }
}
