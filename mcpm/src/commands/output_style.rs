use std::fmt::Display;

use owo_colors::Style;

use crate::terminal;

/// Escape controls in metadata so it cannot change terminal or TSV structure.
pub(super) fn text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() {
                ch.escape_default().to_string()
            } else {
                ch.to_string()
            }
        })
        .collect()
}

pub(super) struct OutputStyle {
    color: bool,
}

impl OutputStyle {
    pub(super) fn new() -> Self {
        Self {
            color: terminal::color_enabled(),
        }
    }

    pub(super) fn plain() -> Self {
        Self { color: false }
    }

    pub(super) fn important(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_white().bold())
    }

    pub(super) fn theme(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_cyan().bold())
    }

    pub(super) fn secondary(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_black())
    }

    pub(super) fn required(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_green().bold())
    }

    pub(super) fn recommended(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_blue().bold())
    }

    pub(super) fn suggested(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_white())
    }

    pub(super) fn warning(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_yellow().bold())
    }

    pub(super) fn danger(&self, value: impl Display) -> String {
        self.paint(value, Style::new().bright_red().bold())
    }

    pub(super) fn loader(&self, id: &str, name: &str) -> String {
        let style = match id {
            "forge" => Style::new().bright_yellow(),
            "neoforge" => Style::new().bright_red(),
            "fabric" => Style::new().bright_cyan(),
            _ => Style::new().bright_blue(),
        };
        self.paint(name, style)
    }

    fn paint(&self, value: impl Display, style: Style) -> String {
        if self.color {
            style.style(value).to_string()
        } else {
            value.to_string()
        }
    }
}
