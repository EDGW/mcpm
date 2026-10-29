//! Shared local and remote error-chain rendering, independent of logging and transport.

use std::io::{self, Write};

use crate::diagnostics::ErrorReport;

use super::{
    console::ConsoleWriter,
    style::{OutputStyle, text},
};

/// Keeps the shared cause chain and adds typed, command-independent detail renderers.
pub(crate) fn diagnostic(
    output: &mut ConsoleWriter<'_>,
    error: &anyhow::Error,
    remote: bool,
) -> io::Result<()> {
    let report = ErrorReport::capture_with(error, |cause| {
        super::backup::verification_message(cause).unwrap_or_else(|| cause.to_string())
    });
    write(output, &report, remote)?;
    super::backup::verification(output, error)
}

/// The diagnostic owner supplies captured causes and decides when stderr is available.
pub(crate) fn write(
    output: &mut ConsoleWriter<'_>,
    error: &ErrorReport,
    remote: bool,
) -> io::Result<()> {
    let style = OutputStyle::stderr();
    let label = if remote { "remote error:" } else { "error:" };
    writeln!(
        output,
        "{} {}",
        style.danger(label),
        style.normal(text(&error.message))
    )?;
    let mut output = output.subwriter();
    for cause in &error.causes {
        writeln!(
            output,
            "{} {}",
            style.danger("caused by:"),
            style.normal(text(cause))
        )?;
    }
    Ok(())
}
