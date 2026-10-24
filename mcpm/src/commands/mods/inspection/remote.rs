//! Inspection command registration, structured SSH requests and business-stream validation.

use std::{num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result};
use mcpm_backend::local::modloader::BUILTIN_LOADERS;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    commands::supports::mods::{
        inspect::{self, Display, Event},
        loaders::LoaderArgs,
        selection::{Scope, Selection, SelectionInput},
    },
    diagnostics::ErrorReport,
    remote::command::{Command, Output},
    ssh::SshArgs,
};

struct Info;
impl Command for Info {
    const ID: &'static str = "mod.info";

    fn run(contents: Value, output: &Output) -> Result<()> {
        serve(contents, output)
    }
}

struct Deps;
impl Command for Deps {
    const ID: &'static str = "mod.deps";

    fn run(contents: Value, output: &Output) -> Result<()> {
        serve(contents, output)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default)]
    selection: SelectionInput,
    #[serde(default = "default_directory")]
    directory: PathBuf,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default = "crate::commands::execution::default_parallelism")]
    parallelism: NonZeroUsize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<mcpm_backend::local::mcmod::ModOpenCache>,
}

fn default_directory() -> PathBuf {
    PathBuf::from(".")
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
fn serve(contents: Value, output: &Output) -> Result<()> {
    tracing::info!("Starting remote mod inspection");
    let decoded = (|| -> Result<_> {
        let args: Request =
            serde_json::from_value(contents).context("invalid mod inspection arguments")?;
        tracing::debug!(?args, "Decoded remote mod inspection arguments");
        let filtered = !args.loaders.is_empty();
        let mut loaders = if filtered {
            Default::default()
        } else {
            BUILTIN_LOADERS.clone()
        };
        for id in args.loaders {
            let loader = BUILTIN_LOADERS
                .get(id.as_str())
                .with_context(|| format!("unavailable mod loader: {id}"))?;
            loaders.insert(loader.id().clone(), loader.clone());
        }
        Ok((
            args.selection.into_selection()?,
            args.directory,
            loaders,
            filtered,
            crate::commands::supports::mods::scan::Options {
                parallelism: args.parallelism.get(),
                cache: args.cache,
            },
        ))
    })();
    let (selection, directory, loaders, filtered, loading) = match decoded {
        Ok(args) => args,
        Err(error) => {
            tracing::debug!(error = %format_args!("{error:#}"), "Rejected invalid mod inspection arguments");
            crate::logging::record_error(&error);
            output.send(Event::Error {
                error: ErrorReport::capture(&error),
            })?;
            return output.send(Event::Done {
                files: 0,
                failed: 1,
                ignored: 0,
                cache: None,
            });
        }
    };
    inspect::load(
        selection,
        Scope::directory(directory),
        loaders,
        filtered,
        loading,
        || output.checkpoint(),
        |event| {
            if let Event::Error { error } = &event {
                crate::logging::record_error(&error.clone().into_error());
            }
            output.send(event)
        },
    )
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run_ssh(
    ssh: SshArgs,
    selection: Selection,
    directory: PathBuf,
    loaders: LoaderArgs,
    loading: crate::commands::supports::mods::scan::Options,
    display: Display<'_>,
) -> Result<()> {
    let command = match display {
        Display::Info => Info::ID,
        Display::Dependencies(_) => Deps::ID,
    };
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
    let args = Request {
        selection: SelectionInput::from_selection(selection)?,
        directory,
        loaders: ids,
        parallelism: NonZeroUsize::new(loading.parallelism)
            .context("inspection parallelism must be positive")?,
        cache: loading.cache,
    };
    inspect::run_ssh(ssh, command, serde_json::to_value(args)?, display)
}

fn init() -> Result<()> {
    crate::remote::registry::register::<Info>()?;
    crate::remote::registry::register::<Deps>()
}

inventory::submit! { crate::initcall::Initcall(init) }
