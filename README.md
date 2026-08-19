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
- show a navigable outline in a dedicated sidebar;
- browse Markdown files from the same sidebar and open them without leaving the workspace;
- render headings, paragraphs, emphasis, links, lists, task lists, quotes, code blocks, tables and thematic breaks;
- render local Markdown and HTML images with terminal-aware sizing;
- reload the active document when it changes on disk;
- switch between documents without leaving the workspace;
- search the rendered document and move between matches;
- use familiar arrow-key controls alongside a small set of vim-inspired shortcuts;
- preserve the terminal emulator's native text selection and copying.

The mouse is intentionally not captured yet. This means the terminal can still do what terminals have been doing since before anyone tried to put a web browser in one: selecting text reliably.

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

When a directory is provided, MarkR recursively discovers files with the `.md`, `.markdown` and `.mdown` extensions and starts with the first one in sorted order.

## Controls

| Key | Action |
| --- | --- |
| `↑` / `↓`, `j` / `k` | Navigate the document or outline |
| `Tab` | Switch focus between outline and document |
| `Enter` | Jump to the selected outline entry |
| `[` / `]` | Previous / next Markdown file |
| `g` / `G` | Go to the top / bottom |
| `Ctrl-u` / `Ctrl-d` | Page up / down |
| `t` | Toggle the outline sidebar |
| `1` / `2` | Switch between outline and files |
| `Enter` while browsing files | Open the selected Markdown file |
| `/` | Search the rendered document |
| `Enter` / `Esc` while searching | Confirm / cancel the search |
| `n` / `N` | Next / previous search match |
| `?` | Open the quick guide |
| `q` | Quit |

The controls are intentionally small. MarkR may borrow a few ideas from modal editors, but it does not require a pilgrimage through a 900-page manual before the first scroll.

## Design direction

MarkR aims for a terminal workspace with the atmosphere of a carefully configured editor:

- a calm dark surface with restrained accent colors;
- generous spacing and clear document hierarchy;
- an outline that makes large documents feel smaller;
- subtle borders and symbols instead of visual noise;
- familiar interactions before clever ones.

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

This separation gives the project room to grow into themes, editing, richer workspace navigation and integrations without turning the event loop into a cupboard full of mystery wires.

## Roadmap

The short-term roadmap includes:

- improve wrapping and viewport-aware outline jumps;
- add syntax highlighting for fenced code;
- add mouse-aware selection and clipboard support;
- add configurable themes and terminal hyperlinks;
- support more Markdown extensions where the terminal allows it;
- explore editing and assisted documentation workflows after the reader experience is solid.

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
