//! SSH-backed list data acquisition, process cleanup and local presentation.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::commands::mod_support::{list, loaders::LoaderArgs, progress::Progress};
use crate::remote::client::Session;

use super::ListDisplayArgs;
use crate::ssh::SshArgs;

mod read;
mod records;

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(
    ssh: SshArgs,
    paths: Vec<PathBuf>,
    loaders: LoaderArgs,
    display: ListDisplayArgs,
) -> Result<()> {
    let started = std::time::Instant::now();
    tracing::info!("Opening SSH data source for mod list");
    let mut session =
        Session::start(&mut ssh.command()?).context("failed to open SSH mcpm session")?;
    let progress = Progress::new();
    let (records, success) = read::read(&session, request(paths, &loaders)?, &progress)
        .context("invalid remote mod list protocol")?;
    drop(progress);
    tracing::info!(
        records = records.len(),
        success,
        elapsed_ms = started.elapsed().as_millis(),
        "Received SSH mod list data"
    );
    let loaded = records::convert(records)?;
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
        loaded.input_errors,
        loaded.failed,
        source_loaders,
        false,
    );
    session.close()?;
    tracing::info!(
        success = result.is_ok(),
        elapsed_ms = started.elapsed().as_millis(),
        "SSH mod list ended"
    );
    result
}

/// Adapt CLI selection to the independent remote argument schema at the frontend boundary.
fn request(paths: Vec<PathBuf>, loaders: &LoaderArgs) -> Result<super::remote::Args> {
    use super::remote::{Args, Loader};
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
    Ok(Args { paths, loaders })
}
