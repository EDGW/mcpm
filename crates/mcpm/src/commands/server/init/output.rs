//! Console-only Git notifications and success messages for server initialization.

use std::io::{self, Write};

use mcpm_backend::operations::server::init::git::{Outcome, Status};
use mcpm_rich::rich;

use crate::{
    display::{
        console::ConsoleWriter,
        rich::block,
        style::{OutputStyle, text},
    },
    terminal,
};

/// Import diagnostics stay on stderr, including Raw mode, leaving server records unchanged.
pub(super) fn write_publication(
    output: &mut impl Write,
    publication: &mcpm_backend::operations::server::init::Publication,
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
        let warning = rich!(
            "[No launch entry is configured; explicitly import a start script before using
             launch.](warning)"
        );
        block(&mut output, &warning, &style)?;
    }
    if !publication.imported.is_empty() && !publication.preserved.is_empty() {
        let warning = rich!(
            "[Existing](warning) `conf.toml` [was preserved; source tuning settings were not
             merged.](warning)"
        );
        block(&mut output, &warning, &style)?;
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

pub(super) fn write_success(output: &mut impl Write) -> io::Result<()> {
    if !terminal::stdout_is_tty() {
        return Ok(());
    }
    let mut output = ConsoleWriter::new(output);
    writeln!(output)?;
    block(
        &mut output,
        &rich!("[Initialized successfully.](success)"),
        &OutputStyle::new(),
    )
}
