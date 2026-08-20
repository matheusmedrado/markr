use std::fmt;
use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeName {
    #[default]
    Markr,
    Midnight,
    Paper,
}

impl ThemeName {
    pub const ALL: [Self; 3] = [Self::Markr, Self::Midnight, Self::Paper];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markr => "markr",
            Self::Midnight => "midnight",
            Self::Paper => "paper",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Markr => Self::Midnight,
            Self::Midnight => Self::Paper,
            Self::Paper => Self::Markr,
        }
    }
}

impl fmt::Display for ThemeName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ThemeName {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|theme| theme.as_str().eq_ignore_ascii_case(value))
            .ok_or_else(|| {
                format!("unknown theme `{value}`; choose one of: markr, midnight, paper")
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: ThemeName,
    pub background: Color,
    pub surface: Color,
    pub surface_active: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub link: Color,
    pub border: Color,
    pub code: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(ThemeName::default())
    }
}

impl Theme {
    pub const fn new(name: ThemeName) -> Self {
        match name {
            ThemeName::Markr => Self {
                name,
                background: Color::Rgb(10, 10, 11),
                surface: Color::Rgb(17, 17, 19),
                surface_active: Color::Rgb(30, 30, 33),
                text: Color::Rgb(232, 232, 232),
                text_muted: Color::Rgb(137, 137, 143),
                accent: Color::Rgb(232, 55, 64),
                link: Color::Rgb(107, 174, 255),
                border: Color::Rgb(53, 53, 59),
                code: Color::Rgb(219, 180, 108),
            },
            ThemeName::Midnight => Self {
                name,
                background: Color::Rgb(7, 12, 20),
                surface: Color::Rgb(12, 20, 32),
                surface_active: Color::Rgb(22, 35, 52),
                text: Color::Rgb(226, 232, 240),
                text_muted: Color::Rgb(125, 141, 163),
                accent: Color::Rgb(86, 182, 194),
                link: Color::Rgb(114, 180, 255),
                border: Color::Rgb(40, 58, 78),
                code: Color::Rgb(222, 180, 110),
            },
            ThemeName::Paper => Self {
                name,
                background: Color::Rgb(244, 241, 234),
                surface: Color::Rgb(234, 229, 219),
                surface_active: Color::Rgb(221, 214, 201),
                text: Color::Rgb(38, 36, 33),
                text_muted: Color::Rgb(105, 99, 90),
                accent: Color::Rgb(178, 48, 57),
                link: Color::Rgb(40, 93, 153),
                border: Color::Rgb(187, 178, 163),
                code: Color::Rgb(142, 91, 35),
            },
        }
    }

    pub const fn next(self) -> Self {
        Self::new(self.name.next())
    }

    pub const fn syntax_theme(self) -> &'static str {
        match self.name {
            ThemeName::Markr | ThemeName::Midnight => "base16-ocean.dark",
            ThemeName::Paper => "InspiredGitHub",
        }
    }

    pub fn title(self) -> Style {
        Style::default().fg(self.text).add_modifier(Modifier::BOLD)
    }

    pub fn muted(self) -> Style {
        Style::default().fg(self.text_muted)
    }

    pub fn accent(self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use ratatui::style::Color;

    use super::{Theme, ThemeName};

    #[test]
    fn parses_theme_names_case_insensitively() {
        assert_eq!(ThemeName::from_str("MIDNIGHT"), Ok(ThemeName::Midnight));
        assert!(ThemeName::from_str("unknown").is_err());
    }

    #[test]
    fn cycles_through_every_theme() {
        assert_eq!(ThemeName::Markr.next(), ThemeName::Midnight);
        assert_eq!(ThemeName::Midnight.next(), ThemeName::Paper);
        assert_eq!(ThemeName::Paper.next(), ThemeName::Markr);
    }

    #[test]
    fn keeps_markr_as_the_default_and_paper_as_a_light_palette() {
        assert_eq!(Theme::default().name, ThemeName::Markr);
        assert_eq!(
            Theme::new(ThemeName::Paper).background,
            Color::Rgb(244, 241, 234)
        );
    }
}
