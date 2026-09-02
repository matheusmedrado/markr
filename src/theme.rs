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
    /// The hairline that separates the sidebar, rules headings and tables.
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
                border: Color::Rgb(107, 99, 89),
                code: Color::Rgb(217, 178, 106),
                reader_border: Color::Rgb(61, 55, 47),
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
                border: Color::Rgb(97, 117, 143),
                code: Color::Rgb(222, 180, 110),
                reader_border: Color::Rgb(51, 68, 90),
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
                reader_border: Color::Rgb(194, 184, 163),
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
            // Ocean is handsome but desaturated; on a warm slab it washes out.
            ThemeName::Markr | ThemeName::Midnight => "base16-eighties.dark",
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
    fn keeps_the_dimmest_tones_off_the_background() {
        // Nothing is painted behind the text, so a user's transparent or
        // blurred terminal shows through. The quietest tones still have to
        // separate from the palette's own ground by a visible margin.
        for name in ThemeName::ALL {
            let theme = Theme::new(name);
            for (label, color) in [
                ("border", theme.border),
                ("reader_border", theme.reader_border),
            ] {
                let (Color::Rgb(r, g, b), Color::Rgb(br, bg, bb)) = (color, theme.background)
                else {
                    panic!("{name} uses a non-RGB {label}");
                };
                let distance =
                    r.abs_diff(br) as u32 + g.abs_diff(bg) as u32 + b.abs_diff(bb) as u32;
                assert!(
                    distance > 60,
                    "{name}'s {label} is too close to its own ground to read"
                );
            }
        }
    }
}
