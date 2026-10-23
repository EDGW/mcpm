//! Typed mod.list command, input discovery and immediate per-file progress/results.

use std::{num::NonZeroUsize, path::PathBuf};

use crate::commands::{execution::DEFAULT_PARALLELISM, mod_support::scan::ScanSession};

use anyhow::{Context, Result};
use kako_mods_lib::local::modloader::BUILTIN_LOADERS;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::remote::command::{Command, Output};

use crate::commands::mod_support::list::protocol;

use protocol::{Contents, Record, Status};

#[derive(Debug)]
pub(super) struct List;

impl Command for List {
    const ID: &'static str = "mod.list";

    fn run(contents: Value, output: &Output) -> Result<()> {
        let started = std::time::Instant::now();
        tracing::info!("Starting remote mod list loading");
        let mut args: Args = match serde_json::from_value(contents) {
            Ok(args) => args,
            Err(error) => {
                tracing::debug!(%error, "Rejected invalid mod list arguments");
                output.send(Contents::Error {
                    error: crate::diagnostics::ErrorReport::capture(
                        &anyhow::Error::from(error).context("invalid list arguments"),
                    ),
                })?;
                return output.send(Contents::Done {
                    success: false,
                    cache: None,
                });
            }
        };
        tracing::debug!(?args, "Decoded remote mod list arguments");
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
                Err(error) => {
                    tracing::debug!(path = %path.display(), error = %format_args!("{error:#}"), "Retaining failed input as a mod list record");
                    inputs.push((path, Err(error)));
                }
            }
        }
        let count = inputs.len();
        tracing::debug!(count, "Remote mod list input discovery completed");
        output.send(Contents::Count {
            count: inputs.len(),
        })?;
        let mut success = true;
        let mut paths = Vec::new();
        let mut indices = Vec::new();
        for (index, (path, entry)) in inputs.into_iter().enumerate() {
            output.checkpoint()?;
            match entry {
                Ok(_) => {
                    paths.push(path);
                    indices.push(index);
                }
                Err(error) => {
                    success = false;
                    output.send(Contents::Item {
                        index,
                        record: Record::failed_input(&path, &error),
                    })?;
                }
            }
        }
        let mut scan = ScanSession::new(selected).with_options(
            crate::commands::mod_support::scan::Options {
                parallelism: args.parallelism.get(),
                cache: args.cache,
            },
        )?;
        scan.batch(&paths, || output.checkpoint(), |position, result, _| {
            let index = indices[position];
            let record = Record::from_scan(&paths[position], result)?;
            tracing::debug!(index, count, path = %record.path, status = ?record.status, errors = record.errors.len(), "Loaded remote mod record");
            success &= record.status != Status::Failed;
            output.send(Contents::Item { index, record })
        })?;
        output.send(Contents::Done {
            success,
            cache: Some(scan.cache_summary()),
        })?;
        tracing::debug!(
            count,
            success,
            elapsed_ms = started.elapsed().as_millis(),
            "Remote mod list loading completed"
        );
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub loaders: Vec<Loader>,
    #[serde(default = "default_parallelism")]
    pub parallelism: NonZeroUsize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<kako_mods_lib::local::mcmod::ModOpenCache>,
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

fn default_parallelism() -> NonZeroUsize {
    NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap()
}

/// Adapt CLI selection to the independent remote argument schema at the frontend boundary.
pub(super) fn request(
    paths: Vec<PathBuf>,
    loaders: &crate::commands::mod_support::loaders::LoaderArgs,
    loading: crate::commands::mod_support::scan::Options,
) -> Result<Args> {
    let mut ids = if loaders.is_filtered() {
        loaders
            .select()
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    ids.sort();
    let loaders = ids
        .iter()
        .map(|id| match id.as_str() {
            "forge" => Ok(Loader::Forge),
            "neoforge" => Ok(Loader::Neoforge),
            "fabric" => Ok(Loader::Fabric),
            _ => anyhow::bail!("unsupported remote loader: {id}"),
        })
        .collect::<Result<_>>()?;
    Ok(Args {
        paths,
        loaders,
        parallelism: std::num::NonZeroUsize::new(loading.parallelism).unwrap(),
        cache: loading.cache,
    })
}
