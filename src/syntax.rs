use std::sync::OnceLock;

use ratatui::style::{Color, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::theme::Theme as MarkrTheme;

struct HighlighterAssets {
    syntax_set: SyntaxSet,
    theme: Theme,
}

static ASSETS: OnceLock<HighlighterAssets> = OnceLock::new();

pub fn highlight(language: Option<&str>, code: &str, theme: MarkrTheme) -> Vec<Vec<Span<'static>>> {
    let assets = ASSETS.get_or_init(load_assets);
    let syntax = find_syntax(language, &assets.syntax_set);
    let mut highlighter = HighlightLines::new(syntax, &assets.theme);

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
    let theme = theme_set
        .themes
        .get("base16-ocean.dark")
        .cloned()
        .or_else(|| theme_set.themes.values().next().cloned())
        .expect("syntect ships at least one default theme");
    HighlighterAssets { syntax_set, theme }
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
    use crate::theme::Theme;

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
}
