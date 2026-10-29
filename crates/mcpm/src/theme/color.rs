//! Validated terminal palette names and adapters for Console, TUI and progress.

use crossterm::style::Color as TuiColor;
use owo_colors::Style;
use serde::{Deserialize, Deserializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Color(u8);

impl Color {
    pub(crate) const CONSOLE: [&'static str; 17] = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright_black",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_white",
        "default_color",
    ];
    pub(crate) const TUI: [&'static str; 17] = [
        "Black",
        "DarkRed",
        "DarkGreen",
        "DarkYellow",
        "DarkBlue",
        "DarkMagenta",
        "DarkCyan",
        "Grey",
        "DarkGrey",
        "Red",
        "Green",
        "Yellow",
        "Blue",
        "Magenta",
        "Cyan",
        "White",
        "Reset",
    ];

    pub(crate) fn parse(value: &str) -> Option<Self> {
        let names = if value.chars().next()?.is_uppercase() {
            &Self::TUI
        } else {
            &Self::CONSOLE
        };
        names
            .iter()
            .position(|name| *name == value)
            .map(|index| Self(index as u8))
    }

    pub(crate) fn tui_name(self) -> &'static str {
        Self::TUI[self.0 as usize]
    }

    pub(crate) fn console(self) -> Style {
        match self.0 {
            0 => Style::new().black(),
            1 => Style::new().red(),
            2 => Style::new().green(),
            3 => Style::new().yellow(),
            4 => Style::new().blue(),
            5 => Style::new().magenta(),
            6 => Style::new().cyan(),
            7 => Style::new().white(),
            8 => Style::new().bright_black(),
            9 => Style::new().bright_red(),
            10 => Style::new().bright_green(),
            11 => Style::new().bright_yellow(),
            12 => Style::new().bright_blue(),
            13 => Style::new().bright_magenta(),
            14 => Style::new().bright_cyan(),
            15 => Style::new().bright_white(),
            _ => Style::new().default_color(),
        }
    }

    pub(crate) fn tui(self) -> TuiColor {
        match self.0 {
            0 => TuiColor::Black,
            1 => TuiColor::DarkRed,
            2 => TuiColor::DarkGreen,
            3 => TuiColor::DarkYellow,
            4 => TuiColor::DarkBlue,
            5 => TuiColor::DarkMagenta,
            6 => TuiColor::DarkCyan,
            7 => TuiColor::Grey,
            8 => TuiColor::DarkGrey,
            9 => TuiColor::Red,
            10 => TuiColor::Green,
            11 => TuiColor::Yellow,
            12 => TuiColor::Blue,
            13 => TuiColor::Magenta,
            14 => TuiColor::Cyan,
            15 => TuiColor::White,
            _ => TuiColor::Reset,
        }
    }

    pub(crate) fn progress(self) -> &'static str {
        // indicatif uses console's color vocabulary rather than owo-colors names.
        const NAMES: [&str; 17] = [
            "black",
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "white",
            "bright.black",
            "bright.red",
            "bright.green",
            "bright.yellow",
            "bright.blue",
            "bright.magenta",
            "bright.cyan",
            "bright.white",
            "",
        ];
        NAMES[self.0 as usize]
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown terminal color {value:?}")))
    }
}
