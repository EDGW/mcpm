//! Shared file-command dispatch for mod inspection through `mods.select`.

use mcpm_backend::data::runtime::RuntimeEnvironment;
use mcpm_backend::mods::selection::SelectionSpec;
use mcpm_backend::mods::types::{ModsCachePolicy, ModsScope};
use mcpm_backend::mods::select::SelectInput;
use std::path::PathBuf;

use anyhow::Result;

use crate::{
    commands::{
        execution::{self, Runtime},
        supports::mods::{
            inspect::{self, Display},
            loaders::LoaderArgs,
            reuse::selection,
            selection::Selection,
        },
    },
    ssh::SshArgs,
};

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(
    selection: Selection,
    directory: PathBuf,
    loaders: LoaderArgs,
    environment: &RuntimeEnvironment,
    loading: impl Into<crate::commands::supports::mods::scan::Options>,
    display: Display<'_>,
    ssh: SshArgs,
) -> Result<()> {
    let loading = loading.into();
    let started = std::time::Instant::now();
    let command = match display {
        Display::Info => "mod.info",
        Display::Dependencies(_) => "mod.deps",
    };
    let _span = tracing::info_span!(
        "mod_inspection",
        command,
        execution = if ssh.is_requested() { "ssh" } else { "local" }
    )
    .entered();
    tracing::info!("Starting mod inspection");
    ssh.validate()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let session = execution::session_for(&ssh)?;
    let request = SelectInput {
        scope: ModsScope::Paths {
            directory,
            paths: Vec::new(),
            loaders: loaders.ids(),
        },
        selection: SelectionSpec::from_selection(selection)?,
        parallelism: loading.parallelism,
        cache: match selection_policy(environment) {
            Some(selection) => ModsCachePolicy::Select { selection },
            None => ModsCachePolicy::Disabled,
        },
        hide_unrecognized: loaders.is_filtered(),
    };
    let runtime = Runtime::new()?;
    let result = inspect::present_operation(&runtime, &session, request, display);
    execution::finish(&runtime, &session)?;
    tracing::debug!(
        success = result.is_ok(),
        elapsed_ms = started.elapsed().as_millis(),
        "Mod inspection ended"
    );
    result
}

fn selection_policy(
    environment: &RuntimeEnvironment,
) -> Option<mcpm_backend::data::cache::mods::ModsCacheSelection> {
    selection(environment)
}
