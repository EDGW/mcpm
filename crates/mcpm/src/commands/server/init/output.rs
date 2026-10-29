//! Console-only Git notifications and success messages for server initialization.

use std::io::{self, Write};

use super::git::{Outcome, Status};
use crate::{
    display::{
        console::ConsoleWriter,
        style::{OutputStyle, text},
    },
    terminal,
};

pub(super) fn write_git(output: &mut impl Write, outcome: Option<&Outcome>) -> io::Result<()> {
    if !terminal::stdout_is_tty() {
        return Ok(());
    }
    let Some(outcome) = outcome else {
        return Ok(());
    };
    let mut output = ConsoleWriter::new(output);
    let style = OutputStyle::new();
    let message = match outcome.status {
        Status::Created => "Created Git repository at ",
        Status::Existing => "Git repository already exists; skipped initialization for ",
    };
    writeln!(
        output,
        "{}{}\n",
        style.normal(message),
        style.normal(text(&outcome.path))
    )
}

pub(super) fn write_success(output: &mut impl Write) -> io::Result<()> {
    if !terminal::stdout_is_tty() {
        return Ok(());
    }
    let mut output = ConsoleWriter::new(output);
    writeln!(
        output,
        "\n{}",
        OutputStyle::new().success("Initialized successfully.")
    )
}
