# MarkR

<p align="center">
  <img src="assets/markr-logo.png" alt="MarkR logo" width="360">
</p>

Markdown deserves better than being opened in a browser tab called `final-final-v7.md`.

*MarkR is being developed as part of a scientific research project (Iniciação Científica) for the Computer Science bachelor's degree at the Universidade Federal de Uberlândia (UFU).*

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
- enter a raw Markdown editor for the active file, edit with familiar cursor controls and save with `Ctrl-S`;
- place the editor cursor with the mouse, undo and redo edits and see Markdown syntax highlighting while editing;
- soft-wrap long lines in the editor rather than scrolling sideways, and move the cursor by what is on screen;
- detect files changed outside MarkR during an edit and offer explicit overwrite or reload choices;
- start with one of three built-in color themes and switch palettes without restarting;
- use familiar arrow-key controls alongside a small set of vim-inspired shortcuts;
- capture mouse input to support selection inside the reader.

## Practical quick start — macOS and Linux

MarkR runs in a regular terminal on macOS or Linux, including macOS Terminal, iTerm2, GNOME
Terminal, Konsole and similar terminals.

### 1. Install Rust

If Rust and Cargo are already installed, skip this step. Otherwise, run the official installer:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Close and reopen the terminal if the `cargo` command is still unavailable.

### 2. Clone the project

```bash
git clone https://github.com/matheusmedrado/markr.git
cd markr
```

### 3. Run MarkR

Open the included README as a test document:

```bash
cargo run -- README.md
```

Once the interface opens, use the mouse wheel or `↑` / `↓` to scroll, `Tab` to switch between the
reader and sidebar, and `q` to quit.

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
| Mouse wheel | Scroll the reader or editor viewport |
| `t` | Toggle the sidebar |
| `T` | Cycle through the built-in color themes |
| `1` / `2` | Switch between outline and files |
| `Enter` or `l` in Files | Enter a directory or open the selected Markdown file |
| `h` or `Backspace` in Files | Go to the parent directory |
| `r` in Files | Refresh the current directory |
| `/` | Search the rendered document |
| `Enter` / `Esc` while searching | Confirm / cancel the search |
| `n` / `N` | Next / previous highlighted search match |
| `e` | Edit the active Markdown file |
| Mouse click in the editor | Place the editor cursor |
| `Ctrl-S` in the editor | Save edits and refresh the rendered document |
| `Ctrl-Z` / `Ctrl-Y` in the editor | Undo / redo the last edit |
| `Esc` in the editor | Return to the reader; confirm when edits are unsaved |
| `q` / `Ctrl-C` in the editor | Quit; confirm when edits are unsaved |
| `s` / `d` at the unsaved prompt | Save / discard changes |
| `o` / `r` at the file-change prompt | Overwrite / reload the file |
| `v` | Start a keyboard selection in the reader |
| `←` / `→`, `↑` / `↓`, `h` / `j` / `k` / `l` while selecting | Extend the selection |
| `y` | Copy the selection to the clipboard |
| Mouse drag in the reader | Select text with the mouse |
| `?` | Open the quick guide |
| `q` | Quit |

The controls are intentionally small. MarkR may borrow a few ideas from modal editors, but it does not require a pilgrimage through a 900-page manual before the first scroll.

## Design direction

MarkR aims for a terminal workspace with the atmosphere of a carefully edited page. The chrome
recedes until the document is the interface: the shell, the sidebar and the reader share a single
plane, and depth comes from typography and spacing rather than from stacked panels:

- calm dark and light palettes with restrained accent colors;
- generous spacing and clear document hierarchy;
- an outline that makes large documents feel smaller;
- structure carried by type and one hairline, not by boxes;
- familiar interactions before clever ones.

Nothing is painted behind the text. The terminal's own background shows through the shell, the
sidebar and the reader, so transparent and blurred terminals keep their backdrop; fills are reserved
for the few things that genuinely need to occlude — code slabs, inline-code chips, search matches and
overlays. Pick the palette that matches your terminal: a light theme in a dark terminal now reads as
dark text on a dark ground, because there is no longer a panel painted underneath it.

There are no panel frames. A single hairline column separates the sidebar from the reader, and the
measure fills the terminal: the reader takes every column it is given, less the gutter and the right
pad, so a wide window is a wide reader rather than a narrow column with empty margins. A ceiling at
two hundred columns exists only as a guard rail for an ultrawide window; an ordinary maximised
terminal never reaches it. Images are sized against that measure and resized with it. Top-level headings close with a hairline rule the way they do on
the web; sub-headings below them are marked by a tick in the four-column gutter at the left of the
measure, so the text column stays flush and free of prefix glyphs. Fenced code sits on a filled slab with a warm
bar instead of inside drawn line art, and tables get one rule under the header rather than a grid —
their cells wrap rather than being cut short. A column is sized to its typical cell rather than its
longest one, so a single long entry wraps instead of holding a channel of whitespace open beside
every other row, and a hairline between records keeps a row trackable across the gap. A table is
sized by its content, so it is centred in the measure rather than left against the margin with the
rest of the line empty beside it. Wrapped source lines reflow into the measure, so paragraphs fill the
column instead of inheriting whatever width the file happened to be written at.
A one-column reading rail at the right edge shows how far through the document you are and doubles as
the reader's focus indicator; the sidebar's hairline does the same for the outline. Overlays — the
quick guide and the save prompts — are frameless panels, and the document behind them redraws at
reduced luminance so it keeps its shape while it is out of focus.

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
- grow the initial raw-source editor into a richer authoring workflow.

The reader remains the default. Editing starts as a focused raw-source mode so authoring can grow
without making the reading experience feel like a small operating system with a Markdown hobby.

## Development

```bash
cargo fmt --all
cargo check
cargo test
cargo clippy -- -D warnings
```

Debug builds compile this crate lightly optimised and its dependencies fully optimised, so
`cargo run` renders at the same speed as a release build. Syntax highlighting and image decoding are
what dominate a frame, and both live in dependencies; leaving them unoptimised made the reader stutter
while scrolling for no reason other than the default profile.

The project keeps its private planning notes in `local/`. That directory is intentionally ignored by Git and is not part of the public project.

## License

MarkR is released under the MIT License.
