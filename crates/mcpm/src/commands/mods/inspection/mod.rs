//! Shared file-command flow over typed Host inspection, without local/SSH business branches.

use crate::{
    commands::supports::mods::{
        inspect::{self, Display},
        loaders::LoaderArgs,
    },
    ssh::SshArgs,
};
use anyhow::Result;
use mcpm_backend::operations::mods::loading::{
    ScopeInput,
    inspection::Input,
    selection::{Selection, SelectionInput},
};
use mcpm_lib::local::cache::mods::ModsCacheSelection;
use std::{num::NonZeroUsize, path::PathBuf};

pub(super) fn run(
    selection: Selection,
    directory: PathBuf,
    loaders: LoaderArgs,
    cache: Option<ModsCacheSelection>,
    parallelism: usize,
    display: Display<'_>,
    ssh: SshArgs,
) -> Result<()> {
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    inspect::run(
        Input {
            scope: ScopeInput::Directory {
                directory,
                loaders: loaders.ids(),
                cache,
            },
            selection: SelectionInput::from_selection(selection)?,
            parallelism: NonZeroUsize::new(parallelism).expect("validated parallelism"),
        },
        display,
        ssh,
    )
}
