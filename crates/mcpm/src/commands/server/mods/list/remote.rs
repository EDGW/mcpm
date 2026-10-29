//! Server-scoped list registration; list acquisition and wire presentation live in shared support.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{num::NonZeroUsize, path::PathBuf};

use super::ServerMods;
use crate::{
    commands::supports::mods::{
        list, scan,
        selection::{Selection, SelectionInput},
    },
    remote::command::{Command, Output},
};

pub(super) struct List;
impl Command for List {
    const ID: &'static str = "server.mods.list";
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    fn run(contents: Value, output: &Output) -> Result<()> {
        let args: Request =
            serde_json::from_value(contents).context("invalid server mod list arguments")?;
        output.checkpoint()?;
        tracing::info!(server = %args.server.display(), "Reading remote server mod scope for list");
        let source = ServerMods::load(args.server)?;
        list::remote_selection::serve(
            args.selection.into_selection()?,
            source.scope(),
            source.loaders(),
            &crate::commands::supports::mods::reuse::QueryArgs {
                no_cache: args.no_cache,
            }
            .environment(source.runtime()),
            scan::Options {
                parallelism: args.parallelism.get(),
            },
            output,
        )
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
    #[serde(default)]
    no_cache: bool,
}
fn default_server() -> PathBuf {
    PathBuf::from(".")
}

pub(super) fn request(
    server: PathBuf,
    selection: Selection,
    no_cache: bool,
    loading: scan::Options,
) -> Result<Value> {
    Ok(serde_json::to_value(Request {
        server,
        selection: SelectionInput::from_selection(selection)?,
        parallelism: NonZeroUsize::new(loading.parallelism)
            .context("list parallelism must be positive")?,
        no_cache,
    })?)
}
fn init() -> Result<()> {
    crate::remote::registry::register::<List>()
}
inventory::submit! { crate::initcall::Initcall(init) }
