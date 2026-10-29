//! Direct invocation through the Windows SSH server's default cmd shell.

use super::{ConnectionError, SUBCOMMAND};

pub(super) fn command(executable: &str) -> Result<String, ConnectionError> {
    // Quotes protect spaces and cmd operators, but not environment expansion.
    // Reject expansion syntax rather than silently changing the executable path.
    if executable.contains(['\0', '\r', '\n', '"', '%', '!']) {
        return Err(ConnectionError::Executable {
            message: "a Windows executable path cannot contain line breaks, double quotes, '%' or '!'"
                .to_owned(),
        });
    }
    Ok(format!("\"{executable}\" {SUBCOMMAND}"))
}
