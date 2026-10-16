//! Shared terminal policy for command output, diagnostics, and interaction.

use std::io::{self, IsTerminal};
use std::sync::OnceLock;

static RAW: OnceLock<bool> = OnceLock::new();

/// Raw output depends only on explicit selection and stdout/stderr terminals.
/// Piped stdin still permits formatting, progress and hints on terminal output.
pub(crate) fn init(raw: bool) {
    RAW.set(raw || !outputs_are_terminals())
        .expect("terminal policy must be initialized once");
}

fn outputs_are_terminals() -> bool {
    io::stdout().is_terminal() && io::stderr().is_terminal()
}

pub(crate) fn is_raw() -> bool {
    RAW.get()
        .copied()
        .unwrap_or_else(|| !outputs_are_terminals())
}

pub(crate) fn is_interactive() -> bool {
    !is_raw()
}

pub(crate) fn color_enabled() -> bool {
    is_interactive() && std::env::var_os("NO_COLOR").is_none()
}
