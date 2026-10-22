//! User-facing error chains rendered with the shared stderr theme policy.

use crate::{
    terminal,
    theme::{self, Role},
};

pub(crate) fn error(error: &anyhow::Error) {
    crate::logging::record_error(error);
    for (depth, cause) in error.chain().enumerate() {
        let indentation = if depth == 0 { "" } else { "  " };
        let label = if depth == 0 { "error:" } else { "caused by:" };
        if terminal::stderr_color_enabled() {
            eprintln!(
                "{indentation}{} {}",
                theme::console(Role::Danger).style(label),
                theme::console(Role::Normal).style(cause)
            );
        } else {
            eprintln!("{indentation}{label} {cause}");
        }
    }
}
