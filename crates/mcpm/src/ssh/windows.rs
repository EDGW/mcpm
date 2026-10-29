//! Direct mcpm invocation through the Windows SSH server's default cmd shell.

use anyhow::{Result, ensure};

pub(super) fn command(executable: &str) -> Result<String> {
    // Quotes protect spaces and cmd operators, but not environment expansion.
    // Reject expansion syntax rather than silently changing the executable path.
    ensure!(
        !executable.contains(['\0', '\r', '\n', '"', '%', '!']),
        "remote mcpm executable for Windows cmd cannot contain NUL, line breaks, double quotes, '%' or '!'"
    );
    Ok(format!("\"{executable}\" remote"))
}
