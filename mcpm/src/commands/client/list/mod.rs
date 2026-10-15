mod output;

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::{
    mc::client::{MinecraftDirectory, MinecraftModLoader, MinecraftVersion},
    modloader::BUILTIN_LOADERS,
};
use thiserror::Error;

use super::{input, output as client_output};

#[derive(Args)]
pub(super) struct ListArgs {
    /// Minecraft root; defaults to the nearest .minecraft ancestor, or the current directory.
    /// Pass . explicitly to force the current directory.
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,
}

#[derive(Clone, Copy)]
enum State {
    Normal,
    Broken,
    Failed,
}

struct Row {
    name: String,
    state: State,
    loaders: Vec<MinecraftModLoader>,
    detail: String,
}

#[derive(Debug, Error)]
enum ListError {
    #[error("failed to inspect mod loaders for {count} client profile(s)")]
    LoaderInspection { count: usize },
}

pub(super) fn run(args: ListArgs) -> Result<()> {
    let input = input::resolve(args.path)?;
    let directory = MinecraftDirectory::new(input.path)?;
    if input.inferred {
        client_output::write_inferred_root(
            &mut io::stderr().lock(),
            directory.path(),
            "mcpm client list .",
        )
        .context("failed to write Minecraft root hint")?;
    }
    let mut rows = Vec::new();
    let mut failed = 0;
    for version in directory.versions()? {
        let name = version.name().to_owned();
        let row = match version {
            MinecraftVersion::Normal(normal) => match normal.mod_loaders(&BUILTIN_LOADERS) {
                Ok(loaders) => {
                    let detail = if loaders.is_empty() {
                        if normal.is_modded() {
                            "unrecognized"
                        } else {
                            "vanilla"
                        }
                    } else {
                        ""
                    };
                    Row {
                        name,
                        state: State::Normal,
                        loaders,
                        detail: detail.to_owned(),
                    }
                }
                Err(error) => {
                    failed += 1;
                    Row {
                        name,
                        state: State::Failed,
                        loaders: Vec::new(),
                        detail: format!("{error:#}"),
                    }
                }
            },
            MinecraftVersion::Broken(broken) => Row {
                name,
                state: State::Broken,
                loaders: Vec::new(),
                detail: client_output::error_chain(broken.error()),
            },
        };
        rows.push(row);
    }
    let mut stdout = io::stdout().lock();
    output::write_list(&mut stdout, &rows).context("failed to write client list")?;
    stdout.flush().context("failed to flush client list")?;
    if failed > 0 {
        return Err(ListError::LoaderInspection { count: failed }.into());
    }
    Ok(())
}
