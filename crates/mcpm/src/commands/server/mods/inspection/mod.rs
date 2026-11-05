//! Server inspection flow; loader, runtime and container resolution remain on the Host.

use super::input::FileArgs;
use crate::{
    commands::supports::mods::{
        inspect::{self, Display},
        reuse::QueryArgs,
    },
    ssh::SshArgs,
};
use anyhow::Result;
use mcpm_backend::operations::mods::loading::{
    ScopeInput, inspection::Input, selection::SelectionInput,
};
use std::num::NonZeroUsize;

pub(super) fn run(
    files: FileArgs,
    query: QueryArgs,
    parallelism: usize,
    display: Display<'_>,
    ssh: SshArgs,
) -> Result<()> {
    let (path, selection) = files.read()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    inspect::run(
        Input {
            scope: ScopeInput::Server {
                path,
                no_cache: query.no_cache,
            },
            selection: SelectionInput::from_selection(selection)?,
            parallelism: NonZeroUsize::new(parallelism).expect("validated parallelism"),
        },
        display,
        ssh,
    )
}
