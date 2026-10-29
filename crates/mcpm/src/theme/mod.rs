//! Shared frontend theme semantics, independent of commands and backend operations.

use std::sync::OnceLock;

use crossterm::style::{Attribute, ContentStyle};
use owo_colors::Style;

use crate::terminal;

mod color;
pub(crate) mod config;
pub(crate) mod widgets;
pub(crate) use color::Color;

static THEME: OnceLock<Theme> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub(crate) enum Role {
    Primary,
    Secondary,
    Important,
    Normal,
    Muted,
    Warning,
    Success,
    Danger,
}

#[derive(Clone, Debug)]
pub(crate) struct Theme {
    pub(crate) config: config::Config,
}

impl Theme {
    pub(crate) fn console(&self, role: Role) -> Style {
        match role {
            Role::Primary => self.config.palette(false).primary.console().bold(),
            Role::Secondary => self.config.palette(false).secondary.console(),
            Role::Important => Style::new().white().bold(),
            Role::Normal => Style::new().white(),
            Role::Muted => Style::new().white().dimmed(),
            Role::Warning => Style::new().bright_yellow().bold(),
            Role::Success => Style::new().bright_green().bold(),
            Role::Danger => Style::new().bright_red().bold(),
        }
    }

    pub(crate) fn tui(&self, role: Role, enabled: bool) -> ContentStyle {
        let mut style = ContentStyle::new();
        if !enabled {
            return style;
        }
        use crossterm::style::Color;
        style.foreground_color = Some(match role {
            Role::Primary => self.config.palette(true).primary.tui(),
            Role::Secondary => self.config.palette(true).secondary.tui(),
            Role::Warning => Color::Yellow,
            Role::Success => Color::Green,
            Role::Danger => Color::Red,
            _ => Color::Grey,
        });
        if matches!(
            role,
            Role::Primary | Role::Important | Role::Warning | Role::Success | Role::Danger
        ) {
            style.attributes.set(Attribute::Bold);
        }
        if matches!(role, Role::Muted) {
            style.attributes.set(Attribute::Dim);
        }
        style
    }

    pub(crate) fn loader(&self, id: &str) -> Style {
        self.config
            .loader_color(false, id)
            .map_or_else(|| self.console(Role::Secondary), Color::console)
    }

    pub(crate) fn tui_loader(&self, id: &str, enabled: bool) -> ContentStyle {
        let mut style = self.tui(Role::Secondary, enabled);
        if enabled && let Some(color) = self.config.loader_color(true, id) {
            style.foreground_color = Some(color.tui());
        }
        style
    }
}

pub(crate) fn init() -> anyhow::Result<()> {
    let config = config::Config::load(&config::directory()?)?;
    THEME
        .set(Theme { config })
        .expect("theme must be initialized once");
    Ok(())
}

pub(crate) fn get() -> &'static Theme {
    THEME.get_or_init(|| Theme {
        config: config::Config::defaults(),
    })
}

pub(crate) fn console(role: Role) -> Style {
    get().console(role)
}

pub(crate) fn tui(role: Role) -> ContentStyle {
    get().tui(role, terminal::stderr_color_enabled())
}

/// Apply the configured primary color only to progress widgets on stderr.
pub(crate) fn progress_template(template: &str) -> String {
    let color = if terminal::stderr_color_enabled() {
        get().config.palette(false).primary.progress()
    } else {
        ""
    };
    if color.is_empty() {
        return template.to_owned();
    }
    template
        .replace("{wide_bar}", &format!("{{wide_bar:.{color}}}"))
        .replace("{spinner}", &format!("{{spinner:.{color}}}"))
}
