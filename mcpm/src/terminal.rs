//! Independent stdout and stderr terminal policies, with an explicit raw override.

use std::io::{self, IsTerminal};
use std::sync::OnceLock;

static RAW: OnceLock<bool> = OnceLock::new();

/// Stores only the explicit override; each output stream is evaluated separately.
pub(crate) fn init(raw: bool) {
    RAW.set(raw)
        .expect("terminal policy must be initialized once");
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
