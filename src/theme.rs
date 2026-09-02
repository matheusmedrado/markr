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
    /// Base color for the themed shell and Markdown layout.
    pub background: Color,
    pub surface: Color,
    pub surface_active: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub link: Color,
    pub border: Color,
    pub code: Color,
    pub reader_background: Color,
    pub reader_border: Color,
    pub chrome_text: Color,
    pub chrome_muted: Color,
    pub selection: Color,
    pub accent_soft: Color,
    /// Warm mid-tone reserved for list bullets: quieter than `accent`, louder than `text_muted`.
    pub marker: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
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
                background: Color::Rgb(11, 10, 9),
                surface: Color::Rgb(21, 19, 15),
                surface_active: Color::Rgb(30, 26, 21),
                text: Color::Rgb(233, 227, 217),
                text_muted: Color::Rgb(139, 129, 117),
                accent: Color::Rgb(242, 125, 38),
                link: Color::Rgb(127, 180, 201),
                border: Color::Rgb(74, 68, 61),
                code: Color::Rgb(217, 178, 106),
                reader_background: Color::Rgb(11, 10, 9),
                reader_border: Color::Rgb(43, 38, 34),
                chrome_text: Color::Rgb(233, 227, 217),
                chrome_muted: Color::Rgb(139, 129, 117),
                selection: Color::Rgb(43, 38, 34),
                accent_soft: Color::Rgb(138, 84, 36),
                marker: Color::Rgb(199, 124, 56),
                error: Color::Rgb(224, 96, 63),
                warning: Color::Rgb(217, 164, 65),
                success: Color::Rgb(127, 174, 127),
            },
            ThemeName::Midnight => Self {
                name,
                background: Color::Rgb(8, 13, 21),
                surface: Color::Rgb(16, 26, 40),
                surface_active: Color::Rgb(22, 35, 52),
                text: Color::Rgb(221, 229, 239),
                text_muted: Color::Rgb(125, 141, 163),
                accent: Color::Rgb(242, 125, 38),
                link: Color::Rgb(114, 180, 255),
                border: Color::Rgb(68, 86, 109),
                code: Color::Rgb(222, 180, 110),
                reader_background: Color::Rgb(8, 13, 21),
                reader_border: Color::Rgb(34, 48, 63),
                chrome_text: Color::Rgb(221, 229, 239),
                chrome_muted: Color::Rgb(125, 141, 163),
                selection: Color::Rgb(34, 48, 63),
                accent_soft: Color::Rgb(122, 74, 31),
                marker: Color::Rgb(199, 124, 56),
                error: Color::Rgb(224, 96, 63),
                warning: Color::Rgb(217, 164, 65),
                success: Color::Rgb(127, 174, 127),
            },
            ThemeName::Paper => Self {
                name,
                background: Color::Rgb(248, 245, 238),
                surface: Color::Rgb(235, 227, 210),
                surface_active: Color::Rgb(224, 215, 196),
                text: Color::Rgb(35, 32, 28),
                text_muted: Color::Rgb(107, 100, 89),
                accent: Color::Rgb(200, 92, 24),
                link: Color::Rgb(47, 111, 143),
                border: Color::Rgb(140, 132, 117),
                code: Color::Rgb(138, 90, 30),
                reader_background: Color::Rgb(248, 245, 238),
                reader_border: Color::Rgb(207, 198, 180),
                chrome_text: Color::Rgb(35, 32, 28),
                chrome_muted: Color::Rgb(107, 100, 89),
                selection: Color::Rgb(224, 215, 196),
                accent_soft: Color::Rgb(217, 154, 94),
                marker: Color::Rgb(166, 96, 28),
                error: Color::Rgb(169, 45, 38),
                warning: Color::Rgb(154, 106, 18),
                success: Color::Rgb(63, 122, 79),
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

    pub fn muted(self) -> Style {
        Style::default().fg(self.chrome_muted)
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
            Color::Rgb(248, 245, 238)
        );
    }

    #[test]
    fn reads_on_a_single_plane_in_every_palette() {
        for name in ThemeName::ALL {
            let theme = Theme::new(name);
            assert_eq!(
                theme.reader_background, theme.background,
                "{name} splits the reader onto a second plane"
            );
        }
    }
}
