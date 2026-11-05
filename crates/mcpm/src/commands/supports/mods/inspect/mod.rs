//! Shared metadata and dependency presentation over Host inspection operations.

use super::source_path::PathArgs;
use anyhow::{Context, Result};
use mcpm_backend::operations::mods::{
    self,
    loading::inspection::{Event, File, Input, Validator},
};
use std::io::{self, Write};

mod deps;
mod info;
mod loading;
mod output;
pub(in crate::commands) use loading::LoadingArgs;
use output::Presentation;

#[derive(Clone, Copy)]
pub(in crate::commands) enum Display<'a> {
    Info,
    Dependencies(&'a PathArgs),
}
impl Display<'_> {
    fn description(self) -> &'static str {
        match self {
            Self::Info => "mod information",
            Self::Dependencies(_) => "dependency declarations",
        }
    }
}

pub(in crate::commands) fn run(
    mut input: Input,
    display: Display<'_>,
    ssh: crate::ssh::SshArgs,
) -> Result<()> {
    crate::commands::execution::Runtime::new()?.block_on(async {
        let executor = super::target::executor(&mut input.scope, ssh).await?;
        let mut stream = match display {
            Display::Info => executor.call::<mods::info::Info>(input).await?,
            Display::Dependencies(_) => executor.call::<mods::deps::Deps>(input).await?,
        };
        let mut presentation = Presentation::new(display, executor.is_remote());
        let mut validator = Validator::default();
        while let Some(event) = stream.next().await {
            let event = event?;
            validator.accept(&event)?;
            presentation.event(event)?;
        }
        validator.finish()?;
        presentation.finish()
    })
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
