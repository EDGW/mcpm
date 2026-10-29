//! Independent stdout and stderr terminal policies, with an explicit raw override.

use std::io::{self, IsTerminal};
use std::sync::OnceLock;

static LIGHT: OnceLock<Option<bool>> = OnceLock::new();

static RAW: OnceLock<bool> = OnceLock::new();

/// Stores only the explicit override; each output stream is evaluated separately.
pub(crate) fn init(raw: bool) {
    RAW.set(raw)
        .expect("terminal policy must be initialized once");
    LIGHT.get_or_init(|| {
        if raw || (!io::stdout().is_terminal() && !io::stderr().is_terminal()) {
            return None;
        }
        terminal_colorsaurus::theme_mode(Default::default())
            .ok()
            .map(|mode| mode == terminal_colorsaurus::ThemeMode::Light)
    });
}

pub(crate) fn is_raw() -> bool {
    RAW.get().copied().unwrap_or(false)
}

pub(crate) fn stdout_is_tty() -> bool {
    !is_raw() && io::stdout().is_terminal()
}

pub(crate) fn stderr_is_tty() -> bool {
    !is_raw() && io::stderr().is_terminal()
}

pub(crate) fn stdout_color_enabled() -> bool {
    stdout_is_tty() && std::env::var_os("NO_COLOR").is_none()
}

pub(crate) fn stderr_color_enabled() -> bool {
    stderr_is_tty() && std::env::var_os("NO_COLOR").is_none()
}

/// None means the terminal could not report its theme; rendering falls back to dark.
pub(crate) fn light_background() -> Option<bool> {
    LIGHT.get().copied().flatten()
}
