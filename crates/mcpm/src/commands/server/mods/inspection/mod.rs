//! Server-scoped inspection through the shared `mods.select` operation.

use anyhow::Result;
use mcpm_backend::mods::selection::SelectionSpec;
use mcpm_backend::mods::types::{ModsCachePolicy, ModsScope};
use mcpm_backend::mods::select::SelectInput;

use super::input::FileArgs;
use super::super::target;
use crate::{
    commands::execution::Runtime,
    commands::supports::mods::{
        inspect::{self, Display},
        scan,
    },
    ssh::SshArgs,
};

pub(super) fn run(
    files: FileArgs,
    query: crate::commands::supports::mods::reuse::QueryArgs,
    loading: scan::Options,
    display: Display<'_>,
    ssh: SshArgs,
) -> Result<()> {
    let (path, selection) = files.read()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let target = target::resolve(path, ssh)?;
    let dependencies = matches!(display, Display::Dependencies(_));
    let request = SelectInput {
        scope: ModsScope::Server {
            path: target.path.clone(),
        },
        selection: SelectionSpec::from_selection(selection)?,
        parallelism: loading.parallelism,
        cache: if dependencies || query.no_cache {
            ModsCachePolicy::Disabled
        } else {
            ModsCachePolicy::ScopeDefault
        },
        hide_unrecognized: true,
    };
    let runtime = Runtime::new()?;
    let result = inspect::present_operation(&runtime, &target.session, request, display);
    crate::commands::execution::finish(&runtime, &target.session)?;
    result
}
