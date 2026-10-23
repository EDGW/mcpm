//! Shared file-command dispatch for local and SSH inspection; no command-specific presentation.

use std::path::PathBuf;

use anyhow::Result;

use crate::{
    commands::mod_support::{
        inspect::{self, Display},
        loaders::LoaderArgs,
        selection::{Scope, Selection},
    },
    ssh::SshArgs,
};

mod remote;

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(
    selection: Selection,
    directory: PathBuf,
    loaders: LoaderArgs,
    loading: impl Into<crate::commands::mod_support::scan::Options>,
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
    tracing::debug!(
        ?selection,
        ?directory,
        ?loaders,
        parallelism = loading.parallelism,
        "Selected mod inspection options"
    );
    ssh.validate()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let result = if ssh.is_requested() {
        remote::run_ssh(ssh, selection, directory, loaders, loading, display)
    } else {
        inspect::run(
            selection,
            Scope::directory(directory),
            loaders.select(),
            loaders.is_filtered(),
            loading,
            display,
        )
    };
    tracing::debug!(
        success = result.is_ok(),
        elapsed_ms = started.elapsed().as_millis(),
        "Mod inspection ended"
    );
    result
}
