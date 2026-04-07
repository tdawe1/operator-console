use ratatui::style::Color;
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Name {
    Dragon = 0,
    Wave = 1,
}

impl Name {
    pub fn all() -> &'static [Name] {
        &[Name::Dragon, Name::Wave]
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Self::Dragon => "kanagawa-dragon",
            Self::Wave => "kanagawa-wave",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Dragon => "Kanagawa Dragon",
            Self::Wave => "Kanagawa Wave",
        }
    }
}

impl FromStr for Name {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dragon" | "kanagawa-dragon" => Ok(Name::Dragon),
            "wave" | "kanagawa-wave" | "terminal" => Ok(Name::Wave),
            _ => Err(format!(
                "unknown theme `{value}`; run --list-themes to see valid theme names"
            )),
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    shell_background: Color,
    panel_background: Color,
    elevated_background: Color,
    text_color: Color,
    muted_text: Color,
    border_color: Color,
    accent_blue: Color,
    accent_cyan: Color,
    accent_green: Color,
    accent_gold: Color,
    accent_pink: Color,
    accent_red: Color,
    selected_background: Color,
    selected_text: Color,
    contrast_text: Color,
}

const DRAGON: Palette = Palette {
    shell_background: Color::Rgb(18, 21, 25),
    panel_background: Color::Rgb(23, 27, 32),
    elevated_background: Color::Rgb(30, 35, 41),
    text_color: Color::Rgb(206, 210, 214),
    muted_text: Color::Rgb(118, 124, 132),
    border_color: Color::Rgb(63, 71, 82),
    accent_blue: Color::Rgb(121, 143, 166),
    accent_cyan: Color::Rgb(114, 151, 158),
    accent_green: Color::Rgb(132, 154, 129),
    accent_gold: Color::Rgb(178, 158, 122),
    accent_pink: Color::Rgb(150, 138, 154),
    accent_red: Color::Rgb(179, 122, 120),
    selected_background: Color::Rgb(54, 68, 86),
    selected_text: Color::Rgb(222, 226, 230),
    contrast_text: Color::Rgb(18, 21, 25),
};

const WAVE: Palette = Palette {
    shell_background: Color::Rgb(18, 20, 25),
    panel_background: Color::Rgb(23, 26, 33),
    elevated_background: Color::Rgb(29, 33, 41),
    text_color: Color::Rgb(212, 216, 222),
    muted_text: Color::Rgb(124, 131, 141),
    border_color: Color::Rgb(62, 70, 82),
    accent_blue: Color::Rgb(118, 141, 167),
    accent_cyan: Color::Rgb(111, 149, 168),
    accent_green: Color::Rgb(136, 162, 129),
    accent_gold: Color::Rgb(183, 160, 120),
    accent_pink: Color::Rgb(155, 141, 158),
    accent_red: Color::Rgb(186, 120, 128),
    selected_background: Color::Rgb(55, 69, 88),
    selected_text: Color::Rgb(226, 230, 234),
    contrast_text: Color::Rgb(18, 20, 25),
};

static CURRENT_THEME: AtomicU8 = AtomicU8::new(Name::Wave as u8);

pub fn default_theme() -> Name {
    Name::Wave
}

pub fn set_theme(name: Name) {
    CURRENT_THEME.store(name as u8, Ordering::Relaxed);
}

pub fn theme_name() -> Name {
    match CURRENT_THEME.load(Ordering::Relaxed) {
        0 => Name::Dragon,
        1 => Name::Wave,
        _ => default_theme(),
    }
}

fn palette_for(name: Name) -> &'static Palette {
    match name {
        Name::Dragon => &DRAGON,
        Name::Wave => &WAVE,
    }
}

fn current_palette() -> &'static Palette {
    palette_for(theme_name())
}

pub fn shell_background() -> Color {
    current_palette().shell_background
}

pub fn panel_background() -> Color {
    current_palette().panel_background
}

pub fn elevated_background() -> Color {
    current_palette().elevated_background
}

pub fn text_color() -> Color {
    current_palette().text_color
}

pub fn muted_text() -> Color {
    current_palette().muted_text
}

pub fn border_color() -> Color {
    current_palette().border_color
}

pub fn accent_blue() -> Color {
    current_palette().accent_blue
}

pub fn accent_cyan() -> Color {
    current_palette().accent_cyan
}

pub fn accent_green() -> Color {
    current_palette().accent_green
}

pub fn accent_gold() -> Color {
    current_palette().accent_gold
}

pub fn accent_pink() -> Color {
    current_palette().accent_pink
}

pub fn accent_red() -> Color {
    current_palette().accent_red
}

pub fn selected_background() -> Color {
    current_palette().selected_background
}

pub fn selected_text() -> Color {
    current_palette().selected_text
}

pub fn contrast_text(_background: Color) -> Color {
    current_palette().contrast_text
}
