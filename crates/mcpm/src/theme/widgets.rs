//! Ratatui adapters for the active semantic theme and loader colors.

use super::{Role, get};
use crate::terminal;
use crossterm::style::{Attribute, ContentStyle};
use ratatui::style::{Color, Modifier, Style};

pub(crate) fn style(role: Role) -> Style {
    content(get().tui(role, terminal::stderr_color_enabled()))
}

pub(crate) fn loader(id: &str) -> Style {
    content(get().tui_loader(id, terminal::stderr_color_enabled()))
}

pub(crate) fn source(id: &str) -> Style {
    let mut result = style(Role::Secondary);
    if terminal::stderr_color_enabled()
        && let Some(value) = get().config.source_color(true, id)
    {
        result = result.fg(color(value.tui()));
    }
    result
}

/// Use a neutral row background while retaining each value's semantic foreground.
pub(crate) fn selection() -> Style {
    if std::env::var_os("NO_COLOR").is_some() {
        return Style::default().add_modifier(Modifier::REVERSED);
    }
    Style::default().bg(if terminal::light_background().unwrap_or(false) {
        Color::Rgb(220, 220, 220)
    } else {
        Color::Rgb(48, 48, 48)
    })
}

fn content(value: ContentStyle) -> Style {
    let mut result = Style::default();
    if let Some(foreground) = value.foreground_color {
        result = result.fg(color(foreground));
    }
    for (attribute, modifier) in [
        (Attribute::Bold, Modifier::BOLD),
        (Attribute::Dim, Modifier::DIM),
    ] {
        if value.attributes.has(attribute) {
            result = result.add_modifier(modifier);
        }
    }
    result
}

pub(crate) fn color(value: crossterm::style::Color) -> Color {
    use crossterm::style::Color as C;
    match value {
        C::Reset => Color::Reset,
        C::Black => Color::Black,
        C::DarkGrey => Color::DarkGray,
        C::Red => Color::LightRed,
        C::DarkRed => Color::Red,
        C::Green => Color::LightGreen,
        C::DarkGreen => Color::Green,
        C::Yellow => Color::LightYellow,
        C::DarkYellow => Color::Yellow,
        C::Blue => Color::LightBlue,
        C::DarkBlue => Color::Blue,
        C::Magenta => Color::LightMagenta,
        C::DarkMagenta => Color::Magenta,
        C::Cyan => Color::LightCyan,
        C::DarkCyan => Color::Cyan,
        C::White => Color::White,
        C::Grey => Color::Gray,
        C::Rgb { r, g, b } => Color::Rgb(r, g, b),
        C::AnsiValue(index) => Color::Indexed(index),
    }
}
