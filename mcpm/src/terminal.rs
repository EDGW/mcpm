//! Shared terminal policy for command output and diagnostics.

use std::io::{self, IsTerminal};
use std::sync::OnceLock;

static RAW: OnceLock<bool> = OnceLock::new();

#[derive(Clone, Copy)]
pub(crate) enum Stream {
    Stdout,
    Stderr,
}

pub(crate) fn init(raw: bool) {
    RAW.set(raw)
        .expect("terminal policy must be initialized once");
}

/// Raw mode treats both output streams as non-TTYs.
pub(crate) fn is_tty(stream: Stream) -> bool {
    if is_raw() {
        return false;
    }
    match stream {
        Stream::Stdout => io::stdout().is_terminal(),
        Stream::Stderr => io::stderr().is_terminal(),
    }
}

pub(crate) fn is_raw() -> bool {
    RAW.get().copied().unwrap_or(false)
}

pub(crate) fn color_enabled(stream: Stream) -> bool {
    is_tty(stream) && std::env::var_os("NO_COLOR").is_none()
}
