use std::sync::OnceLock;

use ratatui::style::{Color, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::theme::Theme as MarkrTheme;

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
                                Style::default()
                                    .fg(Color::Rgb(
                                        style.foreground.r,
                                        style.foreground.g,
                                        style.foreground.b,
                                    ))
                                    .bg(theme.surface),
                            )
                        })
                        .filter(|span| !span.content.is_empty())
                        .collect()
                })
                .unwrap_or_else(|_| {
                    vec![Span::styled(
                        line.to_string(),
                        theme.muted().bg(theme.surface),
                    )]
                })
        })
        .collect()
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
    use super::highlight;
    use crate::theme::{Theme, ThemeName};

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
