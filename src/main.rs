mod app;
mod event;
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
use crossterm::event::{self as terminal_event, Event as CrosstermEvent};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, Message};
use crate::event::map_event;
use crate::workspace::Workspace;

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
    let workspace = Workspace::open(cli.path, io::stdin().is_terminal())?;
    let mut app = App::new(workspace)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let size = terminal.size()?;
    app.update(Message::Resize {
        width: size.width,
        height: size.height,
    });

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    while !app.should_quit() {
        terminal.draw(|frame| view::render(frame, app))?;

        if terminal_event::poll(Duration::from_millis(150))? {
            match terminal_event::read()? {
                CrosstermEvent::Resize(width, height) => {
                    app.update(Message::Resize { width, height })
                }
                event => {
                    if let Some(message) = map_event(event) {
                        app.update(message);
                    }
                }
            }
        } else {
            app.update(Message::Tick);
        }
    }

    Ok(())
}
