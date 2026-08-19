use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug)]
pub struct Theme {
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
        Self {
            background: Color::Rgb(10, 10, 11),
            surface: Color::Rgb(17, 17, 19),
            surface_active: Color::Rgb(30, 30, 33),
            text: Color::Rgb(232, 232, 232),
            text_muted: Color::Rgb(137, 137, 143),
            accent: Color::Rgb(232, 55, 64),
            link: Color::Rgb(107, 174, 255),
            border: Color::Rgb(53, 53, 59),
            code: Color::Rgb(219, 180, 108),
        }
    }
}

impl Theme {
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
