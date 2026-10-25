//! SSH-backed list data acquisition, process cleanup and local presentation.

use anyhow::{Context, Result, bail};
use mcpm_backend::local::runtime::RuntimeEnvironment;

use crate::commands::supports::mods::{list, loaders::LoaderArgs};

use super::ListDisplayArgs;
use crate::progress::Progress;
use crate::ssh::SshArgs;

mod read;
use super::records;

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands) fn run(
    ssh: SshArgs,
    command: &str,
    contents: serde_json::Value,
    loaders: LoaderArgs,
    display: ListDisplayArgs,
    environment: &RuntimeEnvironment,
) -> Result<()> {
    let started = std::time::Instant::now();
    tracing::info!("Opening SSH data source for mod list");
    let mut session = ssh.connect()?;
    let progress = Progress::new("Scanning mod directory...", "Identifying mods");
    let (records, success, cache) = read::read(&session, command, contents, &progress)
        .context("invalid remote mod list protocol")?;
    drop(progress);
    tracing::debug!(
        records = records.len(),
        success,
        elapsed_ms = started.elapsed().as_millis(),
        "Received SSH mod list data"
    );
    let loaded = records::convert(records, true)?;
    tracing::debug!(
        files = loaded.files.len(),
        input_errors = loaded.input_errors,
        failed = loaded.failed,
        "Converted remote records for local presentation"
    );
    if success == loaded.has_errors() {
        bail!("remote mod list completion status disagrees with its records");
    }
    let source_loaders = if loaders.is_filtered() {
        let mut ids: Vec<_> = loaders
            .select()
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        ids.sort();
        ids
    } else {
        Vec::new()
    };
    tracing::debug!("Presenting remote mod list locally");
    let result = list::present(
        loaded.files,
        loaders.is_filtered(),
        display,
        list::Counts {
            input_errors: loaded.input_errors,
            failed: loaded.failed,
            cache,
        },
        source_loaders,
        crate::commands::source_support::identify::Location::Remote(&session, environment),
    );
    session.close()?;
    tracing::debug!(
        success = result.is_ok(),
        elapsed_ms = started.elapsed().as_millis(),
        "SSH mod list ended"
    );
    result
}
