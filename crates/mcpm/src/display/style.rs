//! Console rendering policy and semantic styles shared by command frontends.

use std::fmt::Display;

use owo_colors::Style;

use crate::{
    terminal,
    theme::{self, Role},
};

/// Escape controls in metadata so it cannot change terminal or TSV structure.
pub(crate) fn text(value: &str) -> String {
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

/// Escapes literal backslashes as well as controls so a TSV field remains unambiguous.
pub(crate) fn tsv(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\\' => "\\\\".into(),
            ch if ch.is_control() => ch.escape_default().to_string(),
            ch => ch.to_string(),
        })
        .collect()
}

#[derive(Debug)]
pub(crate) struct OutputStyle {
    color: bool,
}

impl OutputStyle {
    pub(crate) fn new() -> Self {
        Self {
            color: terminal::stdout_color_enabled(),
        }
    }

    pub(crate) fn stderr() -> Self {
        Self {
            color: terminal::stderr_color_enabled(),
        }
    }

    pub(crate) fn plain() -> Self {
        Self { color: false }
    }

    pub(crate) fn important(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Important))
    }

    pub(crate) fn primary(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Primary))
    }

    pub(crate) fn secondary(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Secondary))
    }

    pub(crate) fn muted(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Muted))
    }

    pub(crate) fn success(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Success))
    }

    pub(crate) fn normal(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Normal))
    }

    pub(crate) fn warning(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Warning))
    }

    pub(crate) fn danger(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Danger))
    }

    pub(crate) fn loader(&self, id: &str, name: &str) -> String {
        let style = theme::get().loader(id);
        self.paint(name, style)
    }

    pub(crate) fn paint(&self, value: impl Display, style: Style) -> String {
        if self.color {
            style.style(value).to_string()
        } else {
            value.to_string()
        }
    }
}
