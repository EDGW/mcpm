//! Console rendering policy and semantic styles shared by command frontends.

use std::fmt::Display;

use owo_colors::Style;

use crate::{
    terminal,
    theme::{self, Role},
};

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
            color: terminal::stdout_color_enabled(),
        }
    }

    pub(super) fn stderr() -> Self {
        Self {
            color: terminal::stderr_color_enabled(),
        }
    }

    pub(super) fn plain() -> Self {
        Self { color: false }
    }

    pub(super) fn important(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Important))
    }

    pub(super) fn primary(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Primary))
    }

    pub(super) fn secondary(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Secondary))
    }

    pub(super) fn muted(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Muted))
    }

    pub(super) fn success(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Success))
    }

    pub(super) fn normal(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Normal))
    }

    pub(super) fn warning(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Warning))
    }

    pub(super) fn danger(&self, value: impl Display) -> String {
        self.paint(value, theme::console(Role::Danger))
    }

    pub(super) fn loader(&self, id: &str, name: &str) -> String {
        let style = theme::get().loader(id);
        self.paint(name, style)
    }

    pub(super) fn paint(&self, value: impl Display, style: Style) -> String {
        if self.color {
            style.style(value).to_string()
        } else {
            value.to_string()
        }
    }
}
