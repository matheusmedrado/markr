# MarkR

<p align="center">
  <img src="assets/markr-logo.png" alt="MarkR logo" width="360">
</p>

Markdown deserves better than being opened in a browser tab called `final-final-v7.md`.

MarkR is a terminal-first Markdown workspace for reading documentation with focus, structure and a little bit of personality. It is built in Rust with `ratatui` and `pulldown-cmark`, following The Elm Architecture from the first line of code.

The goal is simple: make long-form Markdown feel good to read without leaving the terminal. The terminal is already where the work happens. The documentation can come along for the ride.

## Current status

MarkR is an early, actively evolving project. The first milestone is a polished read-only workspace with a document outline, keyboard navigation and automatic file reloads.

The current version can:

- open a Markdown file, a directory of Markdown files or piped stdin;
- discover Markdown files recursively inside a workspace;
- show a navigable outline and file browser in a transparent, responsive sidebar;
- navigate the filesystem from the sidebar, enter and leave directories and open Markdown files from anywhere the process can read;
- render headings, paragraphs, emphasis, links, lists, task lists, quotes, code blocks, tables and thematic breaks;
- render local Markdown and HTML images with terminal-aware sizing;
- reload the active document when it changes on disk;
- switch between documents without leaving the workspace;
- search the rendered document with visible highlights for every match and a distinct active result;
- select and copy text inside the reader with the keyboard or mouse;
- start with one of three built-in color themes and switch palettes without restarting;
- use familiar arrow-key controls alongside a small set of vim-inspired shortcuts;
- capture mouse input to support selection inside the reader.

## Installation

Rust and Cargo are required. Once the project is cloned, run it directly with Cargo:

```bash
cargo run -- README.md
```

You can also open a workspace directory or pipe a document through stdin:

```bash
cargo run -- ./docs
cat README.md | cargo run
```

MarkR ships with `markr`, `midnight` and `paper`. Choose one when opening the app, or press `T` whenever the walls need repainting:

```bash
cargo run -- --theme midnight README.md
```

When a directory is provided, MarkR recursively discovers files with the `.md`, `.markdown` and `.mdown` extensions and starts with the first one in sorted order.

## Controls

| Key | Action |
| --- | --- |
| `↑` / `↓`, `j` / `k` | Navigate the document or active sidebar panel |
| `Tab` | Switch focus between the document and sidebar |
| `↑` / `↓` in sidebar | Move the visible selection in Outline or Files |
| `←` / `→` in sidebar | Switch between Outline and Files |
| `Enter` | Open or activate the selected entry |
| `Esc` | Close help/sidebar overlay and return to the document |
| `[` / `]` | Previous / next Markdown file |
| `g` / `G` | Go to the top / bottom |
| `Ctrl-u` / `Ctrl-d` | Page up / down |
| `t` | Toggle the sidebar |
| `T` | Cycle through the built-in color themes |
| `1` / `2` | Switch between outline and files |
| `Enter` or `l` in Files | Enter a directory or open the selected Markdown file |
| `h` or `Backspace` in Files | Go to the parent directory |
| `r` in Files | Refresh the current directory |
| `/` | Search the rendered document |
| `Enter` / `Esc` while searching | Confirm / cancel the search |
| `n` / `N` | Next / previous highlighted search match |
| `v` | Start a keyboard selection in the reader |
| `←` / `→`, `↑` / `↓`, `h` / `j` / `k` / `l` while selecting | Extend the selection |
| `y` | Copy the selection to the clipboard |
| Mouse drag in the reader | Select text with the mouse |
| `?` | Open the quick guide |
| `q` | Quit |

The controls are intentionally small. MarkR may borrow a few ideas from modal editors, but it does not require a pilgrimage through a 900-page manual before the first scroll.

## Design direction

MarkR aims for a terminal workspace with the atmosphere of a carefully configured editor. The
application uses a fully themed shell with distinct surfaces for the background, sidebar and reader.
Rounded borders and restrained contrast keep the panels connected without making the terminal feel
like a wall of boxes:

- calm dark and light palettes with restrained accent colors;
- generous spacing and clear document hierarchy;
- an outline that makes large documents feel smaller;
- subtle borders and symbols instead of visual noise;
- familiar interactions before clever ones.

The reader is a rounded solid panel with one cell of breathing room on every side on medium and wide
terminals. A short orange editorial marker indicates document focus, while the sidebar uses a quieter
surface and border to keep navigation visually separate from the document.
Below 100 columns the sidebar becomes an overlay; below 72 columns it becomes a temporary full-screen
panel. Choose `markr` or `midnight` for dark terminal backgrounds and `paper` for light ones.

The terminal emulator controls the actual font, so MarkR does not try to change it behind the user's back. The visual identity comes from palette, spacing, symbols, borders and hierarchy. A clean monospace font with good Unicode support will provide the best result.

## Architecture

MarkR follows The Elm Architecture (TEA):

```text
event -> Message -> update(Model) -> view(Model) -> terminal
```

The Markdown parser produces a project-owned intermediate representation before anything reaches the terminal renderer. Parsing, application state, layout and presentation therefore remain separate concerns:

```text
Markdown source
      ↓
pulldown-cmark events
      ↓
MarkR document model
      ↓
layout and styled terminal lines
      ↓
ratatui view
```

This separation gives the project room to grow into editing, richer workspace navigation and integrations without turning the event loop into a cupboard full of mystery wires.

## Roadmap

The short-term roadmap includes:

- add terminal hyperlinks and lightweight document diagnostics;
- improve performance for very large documents and workspaces;
- support more Markdown extensions where the terminal allows it;
- explore editing and richer authoring workflows after the reader experience is solid.

The reader comes first. A tool that cannot make reading pleasant has no business trying to become an editor, or a small operating system with a Markdown hobby.

## Development

```bash
cargo fmt --all
cargo check
cargo test
cargo clippy -- -D warnings
```

The project keeps its private planning notes in `local/`. That directory is intentionally ignored by Git and is not part of the public project.

## License

MarkR is released under the MIT License.
