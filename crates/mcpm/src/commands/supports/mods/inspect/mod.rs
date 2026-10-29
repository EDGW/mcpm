//! Shared inspection presentation for file and server commands.

use std::{
    io::{self, Write},
};

use anyhow::{Context, Result};
use mcpm_backend::mods::select::SelectInput;
use mcpm_backend::mods::Select;

use super::source_path::PathArgs;
use crate::commands::execution::{failure as operation_failure, Runtime};

mod deps;
mod info;
mod loading;
mod model;
mod output;
pub(in crate::commands) use loading::LoadingArgs;
pub(in crate::commands) use model::{Event, File};
pub(in crate::commands) use output::Presentation;

/// Selects the presentation without changing identification or input semantics.
#[derive(Clone, Copy)]
pub(in crate::commands) enum Display<'a> {
    Info,
    Dependencies(&'a PathArgs),
}

impl Display<'_> {
    pub(in crate::commands) fn description(self) -> &'static str {
        match self {
            Self::Info => "mod information",
            Self::Dependencies(_) => "dependency declarations",
        }
    }
}

/// Runs one selection operation and renders its events; the caller owns the request.
pub(in crate::commands) fn present_operation(
    runtime: &Runtime,
    session: &mcpm_backend::Session,
    request: SelectInput,
    display: Display<'_>,
) -> Result<()> {
    let remote = session.target().is_remote();
    let mut presentation = Presentation::new(display, remote);
    runtime.block_on(async {
        let mut handle = session
            .start::<Select>(request)
            .await
            .map_err(|failure| operation_failure(failure, remote))?;
        while let Some(event) = handle.next().await {
            presentation.event(event)?;
        }
        handle
            .finish()
            .await
            .map_err(|failure| operation_failure(failure, remote))?;
        Ok::<(), anyhow::Error>(())
    })?;
    presentation.finish()
}

/// Empty selection succeeds without scanning; interactive users receive guidance.
pub(in crate::commands) fn prompt_for_input() -> Result<()> {
    if crate::terminal::stderr_is_tty() {
        writeln!(
            io::stderr().lock(),
            "Specify at least one file, or use --all to inspect all mods."
        )
        .context("failed to write empty input hint")?;
    }
    Ok(())
}
