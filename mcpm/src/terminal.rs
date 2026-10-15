//! Shared terminal policy for command output, diagnostics, and interaction.

use std::io::{self, IsTerminal};
use std::sync::OnceLock;

static RAW: OnceLock<bool> = OnceLock::new();

pub(crate) fn init(raw: bool) {
    RAW.set(raw || !stdio_are_terminals())
        .expect("terminal policy must be initialized once");
}

fn stdio_are_terminals() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal()
}

pub(crate) fn is_raw() -> bool {
    RAW.get().copied().unwrap_or_else(|| !stdio_are_terminals())
}

pub(crate) fn is_interactive() -> bool {
    !is_raw()
}

pub(crate) fn color_enabled() -> bool {
    is_interactive() && std::env::var_os("NO_COLOR").is_none()
}
