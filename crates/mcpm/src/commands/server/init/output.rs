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

/// Import diagnostics stay on stderr, including Raw mode, leaving server records unchanged.
pub(super) fn write_publication(
    output: &mut impl Write,
    publication: &mcpm_backend::local::mc::server::config::Published,
) -> io::Result<()> {
    let style = OutputStyle::stderr();
    let mut output = ConsoleWriter::new(output);
    for (action, paths) in [
        ("Created", &publication.created),
        ("Replaced", &publication.replaced),
        ("Preserved", &publication.preserved),
        ("Removed", &publication.removed),
    ] {
        for path in paths {
            writeln!(
                output,
                "{} {}",
                style.muted(action),
                style.muted(text(&path.display().to_string()))
            )?;
        }
    }
    if !publication.entry_configured {
        writeln!(output, "{}", style.warning("No launch entry is configured; explicitly import a start script before using launch."))?;
    }
    if !publication.imported.is_empty() && !publication.preserved.is_empty() {
        writeln!(
            output,
            "{}",
            style.warning(
                "Existing conf.toml was preserved; source tuning settings were not merged."
            )
        )?;
    }
    Ok(())
}

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
