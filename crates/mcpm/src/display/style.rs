//! Console rendering policy and semantic styles shared by command frontends.

use mcpm_rich::Role;
use std::fmt::Display;

use owo_colors::Style;

use crate::{terminal, theme};

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

    /// Never colored: in-memory layouts such as TUI diagnostic lines.
    pub(crate) fn plain() -> Self {
        Self { color: false }
    }

    /// Whether styles reach the terminal; plain output must keep textual markers instead.
    pub(crate) fn colored(&self) -> bool {
        self.color
    }

    /// The single role-to-style entry; the named methods below are shorthands for it.
    pub(crate) fn role(&self, role: Role, value: impl Display) -> String {
        self.paint(value, theme::console(role))
    }

    pub(crate) fn important(&self, value: impl Display) -> String {
        self.role(Role::Important, value)
    }

    pub(crate) fn primary(&self, value: impl Display) -> String {
        self.role(Role::Primary, value)
    }

    /// Primary foreground without bold: commands, paths, file names and configuration keys.
    pub(crate) fn code(&self, value: impl Display) -> String {
        self.role(Role::Code, value)
    }

    pub(crate) fn secondary(&self, value: impl Display) -> String {
        self.role(Role::Secondary, value)
    }

    pub(crate) fn muted(&self, value: impl Display) -> String {
        self.role(Role::Muted, value)
    }

    pub(crate) fn success(&self, value: impl Display) -> String {
        self.role(Role::Success, value)
    }

    pub(crate) fn normal(&self, value: impl Display) -> String {
        self.role(Role::Normal, value)
    }

    pub(crate) fn warning(&self, value: impl Display) -> String {
        self.role(Role::Warning, value)
    }

    pub(crate) fn danger(&self, value: impl Display) -> String {
        self.role(Role::Danger, value)
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
