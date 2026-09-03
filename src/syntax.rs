use std::sync::OnceLock;

use ratatui::style::{Color, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::theme::{Theme as MarkrTheme, ThemeName};

struct HighlighterAssets {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

static ASSETS: OnceLock<HighlighterAssets> = OnceLock::new();

pub fn highlight(language: Option<&str>, code: &str, theme: MarkrTheme) -> Vec<Vec<Span<'static>>> {
    let assets = ASSETS.get_or_init(load_assets);
    let syntax = find_syntax(language, &assets.syntax_set);
    let syntax_theme = assets
        .theme_set
        .themes
        .get(theme.syntax_theme())
        .or_else(|| assets.theme_set.themes.get("base16-ocean.dark"))
        .or_else(|| assets.theme_set.themes.values().next())
        .expect("syntect ships at least one default theme");
    let mut highlighter = HighlightLines::new(syntax, syntax_theme);

    LinesWithEndings::from(code)
        .map(|line| {
            highlighter
                .highlight_line(line, &assets.syntax_set)
                .map(|spans| {
                    spans
                        .into_iter()
                        .map(|(style, text)| {
                            Span::styled(
                                text.trim_end_matches(['\r', '\n']).to_string(),
                                // No background: the caller decides whether
                                // these sit on a slab or on the terminal.
                                Style::default().fg(Color::Rgb(
                                    style.foreground.r,
                                    style.foreground.g,
                                    style.foreground.b,
                                )),
                            )
                        })
                        .filter(|span| !span.content.is_empty())
                        .collect()
                })
                .unwrap_or_else(|_| vec![Span::styled(line.to_string(), theme.muted())])
        })
        .collect()
}

/// The editor's Markdown highlighting, held between frames.
///
/// Highlighting is a whole-document job — syntect carries parser state from
/// one line to the next — so the editor used to pay for the entire file on
/// every draw, including the draws where nothing had changed. The cache keeps
/// the result until the text or the palette actually moves, which turns a
/// per-frame cost into a per-keystroke one.
#[derive(Debug, Default)]
pub struct HighlightCache {
    /// The buffer revision and palette the held lines were built from.
    built_from: Option<(u64, ThemeName)>,
    lines: Vec<Vec<Span<'static>>>,
}

impl HighlightCache {
    /// Whether the held lines still describe `revision` under `theme`.
    pub fn is_current(&self, revision: u64, theme: ThemeName) -> bool {
        self.built_from == Some((revision, theme))
    }

    /// Re-highlights `text`, which is expected to be revision `revision` of
    /// the editor buffer. Callers should check [`Self::is_current`] first.
    pub fn rebuild(&mut self, revision: u64, theme: MarkrTheme, text: &str) {
        self.lines = highlight(Some("markdown"), text, theme);

        // `LinesWithEndings` yields nothing after a trailing newline, but the
        // buffer still has an empty line there. Pad so an index into these
        // lines is an index into the buffer.
        if text.is_empty() || text.ends_with('\n') {
            self.lines.push(Vec::new());
        }

        self.built_from = Some((revision, theme.name));
    }

    /// One entry per line of the buffer it was built from.
    pub fn lines(&self) -> &[Vec<Span<'static>>] {
        &self.lines
    }

    pub fn clear(&mut self) {
        self.built_from = None;
        self.lines = Vec::new();
    }
}

fn load_assets() -> HighlighterAssets {
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = ThemeSet::load_defaults();
    HighlighterAssets {
        syntax_set,
        theme_set,
    }
}

fn find_syntax<'a>(language: Option<&str>, syntax_set: &'a SyntaxSet) -> &'a SyntaxReference {
    language
        .and_then(|language| {
            syntax_set
                .find_syntax_by_token(language)
                .or_else(|| syntax_set.find_syntax_by_extension(language))
        })
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text())
}

#[cfg(test)]
mod tests {
    use super::{HighlightCache, highlight};
    use crate::theme::{Theme, ThemeName};

    #[test]
    fn holds_highlighting_until_the_revision_or_palette_moves() {
        let mut cache = HighlightCache::default();
        assert!(!cache.is_current(1, ThemeName::Markr));

        cache.rebuild(1, Theme::default(), "# Title\n");
        assert!(cache.is_current(1, ThemeName::Markr));
        // A later edit, and the same text under another palette, both miss.
        assert!(!cache.is_current(2, ThemeName::Markr));
        assert!(!cache.is_current(1, ThemeName::Paper));
    }

    #[test]
    fn keeps_one_entry_per_buffer_line_including_a_trailing_empty_one() {
        let mut cache = HighlightCache::default();

        // "a\n" is two buffer lines: "a" and the empty one after it.
        cache.rebuild(1, Theme::default(), "a\n");
        assert_eq!(cache.lines().len(), 2);

        cache.rebuild(2, Theme::default(), "a\nb");
        assert_eq!(cache.lines().len(), 2);

        cache.rebuild(3, Theme::default(), "");
        assert_eq!(cache.lines().len(), 1);
    }

    #[test]
    fn clearing_drops_the_held_lines() {
        let mut cache = HighlightCache::default();
        cache.rebuild(1, Theme::default(), "# Title\n");
        cache.clear();

        assert!(cache.lines().is_empty());
        assert!(!cache.is_current(1, ThemeName::Markr));
    }

    #[test]
    fn highlights_known_languages() {
        let lines = highlight(
            Some("rust"),
            "fn main() {}\nlet value = 1;\n",
            Theme::default(),
        );

        assert_eq!(lines.len(), 2);
        assert!(lines[0].len() > 1);
    }

    #[test]
    fn falls_back_to_plain_text_for_unknown_languages() {
        let lines = highlight(
            Some("not-a-real-language"),
            "plain text\n",
            Theme::default(),
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0]
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "plain text"
        );
    }

    #[test]
    fn uses_a_light_syntax_palette_for_the_paper_theme() {
        let code = "fn main() { let answer = 42; }\n";
        let dark = highlight(Some("rust"), code, Theme::default());
        let light = highlight(Some("rust"), code, Theme::new(ThemeName::Paper));
        let foregrounds = |lines: &[Vec<ratatui::text::Span<'static>>]| {
            lines
                .iter()
                .flatten()
                .map(|span| span.style.fg)
                .collect::<Vec<_>>()
        };

        assert_ne!(foregrounds(&dark), foregrounds(&light));
    }

    #[test]
    fn highlights_markdown_sources_for_the_editor() {
        let lines = highlight(Some("markdown"), "# Title\n**bold**\n", Theme::default());

        assert_eq!(lines.len(), 2);
        assert!(lines.iter().flatten().count() > 2);
    }
}
