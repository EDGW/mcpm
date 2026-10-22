//! Typed mod.list command, input discovery and immediate per-file progress/results.

use std::path::PathBuf;

use anyhow::{Context, Result};
use kako_mods_lib::local::modloader::BUILTIN_LOADERS;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::remote::command::{Command, Output};

pub(super) mod protocol;

use protocol::{Contents, Record, Status};

#[derive(Debug)]
pub(super) struct List;

impl Command for List {
    const ID: &'static str = "mod.list";

    fn run(contents: Value, output: &Output) -> Result<()> {
        let mut args: Args = match serde_json::from_value(contents) {
            Ok(args) => args,
            Err(error) => {
                output.send(Contents::Error {
                    message: format!("invalid list arguments: {error}"),
                })?;
                return output.send(Contents::Done { success: false });
            }
        };
        let mut selected = if args.loaders.is_empty() {
            BUILTIN_LOADERS.clone()
        } else {
            Default::default()
        };
        for id in args.loaders {
            let loader = BUILTIN_LOADERS
                .get(id.id())
                .with_context(|| format!("unavailable mod loader: {}", id.id()))?;
            selected.insert(loader.id().clone(), loader.clone());
        }
        if args.paths.is_empty() {
            args.paths.push(PathBuf::from("."));
        }
        let mut inputs = Vec::new();
        for path in args.paths {
            output.checkpoint()?;
            match super::input::read(path.clone()) {
                Ok(entries) => inputs.extend(
                    entries
                        .into_iter()
                        .map(|entry| (entry.path().to_owned(), Ok(entry))),
                ),
                Err(error) => inputs.push((path, Err(error))),
            }
        }
        output.send(Contents::Count {
            count: inputs.len(),
        })?;
        let mut success = true;
        for (index, (path, entry)) in inputs.into_iter().enumerate() {
            output.checkpoint()?;
            let record = Record::load(path, entry, &selected);
            success &= record.status != Status::Failed;
            output.send(Contents::Item { index, record })?;
        }
        output.send(Contents::Done { success })
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub loaders: Vec<Loader>,
}

/// Remote values are independent of Clap option names and CLI argument layout.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub(super) enum Loader {
    Forge,
    Neoforge,
    Fabric,
}

impl Loader {
    fn id(&self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::Neoforge => "neoforge",
            Self::Fabric => "fabric",
        }
    }
}

fn init() -> Result<()> {
    crate::remote::registry::register::<List>()
}

inventory::submit! { crate::initcall::Initcall(init) }
