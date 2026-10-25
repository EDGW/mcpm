//! Shared inspection acquisition, data and local presentation for file and server commands.

use mcpm_backend::local::runtime::RuntimeEnvironment;
use std::{
    collections::HashMap,
    io::{self, Write},
};

use anyhow::{Context, Result};
use mcpm_backend::local::modloader::{LoaderId, ModLoaderMeta};

use super::{
    selection::{Scope, Selection},
    source_path::PathArgs,
};

mod deps;
mod info;
mod load;
mod loading;
mod model;
mod output;
mod remote;
pub(in crate::commands) use output::Presentation;
pub(in crate::commands) use remote::run_ssh;
mod validation;
pub(in crate::commands) use validation::Validator;

pub(in crate::commands) use load::load;
pub(in crate::commands) use loading::LoadingArgs;
pub(in crate::commands) use model::{Event, File};

/// Selects the presentation without changing identification or input semantics.
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

/// The local frontend consumes the same acquisition events as the SSH frontend.
#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands) fn run(
    selection: Selection,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    hide_unrecognized: bool,
    environment: &RuntimeEnvironment,
    loading: impl Into<super::scan::Options>,
    display: Display<'_>,
) -> Result<()> {
    let mut presentation = Presentation::new(display, false);
    let mut validator = Validator::default();
    load(
        selection,
        scope,
        loaders,
        hide_unrecognized,
        environment,
        loading,
        || Ok(()),
        |event| {
            validator.accept(&event)?;
            presentation.event(event)
        },
    )?;
    validator.finish()?;
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
