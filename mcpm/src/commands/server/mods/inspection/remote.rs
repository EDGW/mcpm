//! Server inspection command schemas and handlers; shared inspection owns event production and consumption.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{num::NonZeroUsize, path::PathBuf};

use super::super::super::mod_source::ServerMods;
use crate::{
    commands::supports::mods::{
        inspect::{self, Display, Event},
        scan,
        selection::{Selection, SelectionInput},
    },
    remote::command::{Command, Output},
    ssh::SshArgs,
};

struct Info;
impl Command for Info {
    const ID: &'static str = "server.mods.info";
    fn run(contents: Value, output: &Output) -> Result<()> {
        serve(contents, output)
    }
}
struct Deps;
impl Command for Deps {
    const ID: &'static str = "server.mods.deps";
    fn run(contents: Value, output: &Output) -> Result<()> {
        serve(contents, output)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default = "default_server")]
    server: PathBuf,
    #[serde(default)]
    selection: SelectionInput,
    #[serde(default = "crate::commands::execution::default_parallelism")]
    parallelism: NonZeroUsize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<mcpm_backend::local::mcmod::ModOpenCache>,
}
fn default_server() -> PathBuf {
    PathBuf::from(".")
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
fn serve(contents: Value, output: &Output) -> Result<()> {
    let args: Request =
        serde_json::from_value(contents).context("invalid server mod inspection arguments")?;
    output.checkpoint()?;
    tracing::info!(server = %args.server.display(), "Reading remote server mod scope for inspection");
    let source = ServerMods::load(args.server)?;
    inspect::load(
        args.selection.into_selection()?,
        source.scope(),
        source.loaders(),
        true,
        scan::Options {
            parallelism: args.parallelism.get(),
            cache: args.cache,
        },
        || output.checkpoint(),
        |event| {
            if let Event::Error { error } = &event {
                crate::logging::record_error(&error.clone().into_error());
            }
            output.send(event)
        },
    )
}

pub(super) fn run(
    ssh: SshArgs,
    server: PathBuf,
    selection: Selection,
    loading: scan::Options,
    display: Display<'_>,
) -> Result<()> {
    let command = match display {
        Display::Info => Info::ID,
        Display::Dependencies(_) => Deps::ID,
    };
    inspect::run_ssh(
        ssh,
        command,
        serde_json::to_value(Request {
            server,
            selection: SelectionInput::from_selection(selection)?,
            parallelism: NonZeroUsize::new(loading.parallelism)
                .context("inspection parallelism must be positive")?,
            cache: loading.cache,
        })?,
        display,
    )
}
fn init() -> Result<()> {
    crate::remote::registry::register::<Info>()?;
    crate::remote::registry::register::<Deps>()
}
inventory::submit! { crate::initcall::Initcall(init) }
