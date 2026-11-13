//! Shared Console presentation of Host-side Git initialization results.

use crate::{
    display::{console::ConsoleWriter, rich::block, style::OutputStyle},
    terminal,
};
use mcpm_backend::operations::server::init::git::{Outcome, Status};
use mcpm_rich::rich;
use std::io::{self, Write};
pub(super) fn write_git(output: &mut impl Write, outcome: Option<&Outcome>) -> io::Result<()> {
    if !terminal::stdout_is_tty() {
        return Ok(());
    }
    let Some(outcome) = outcome else {
        return Ok(());
    };
    let mut output = ConsoleWriter::new(output);
    let path = &outcome.path;
    let message = match outcome.status {
        Status::Created => rich!("Created Git repository at `{path}`"),
        Status::Existing => {
            rich!("Git repository already exists; skipped initialization for `{path}`")
        }
    };
    block(&mut output, &message, &OutputStyle::new())?;
    writeln!(output)
}
