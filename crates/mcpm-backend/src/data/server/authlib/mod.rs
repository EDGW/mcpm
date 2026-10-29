//! Managed authlib-injector JAR under `.mcpm/authlib-injector`: policy, verification,
//! download and atomic publication.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use kako_mc_lib::download::{Downloader, authlib_injector};

mod metadata;
mod storage;

#[cfg(test)]
mod tests;

/// Per-launch policy for the managed JAR, unrelated to the authentication API URL.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum JarPolicy {
    #[default]
    Auto,
    Local,
    Refresh,
}

/// Preparation events; download events keep their generic transport meaning.
#[derive(Debug, Clone, Copy)]
pub enum Event<'a> {
    Checking,
    Resolving,
    Download(kako_mc_lib::download::Event<'a>),
    Installing,
    Ready,
}

/// The sole managed JAR location, also used for argument preflight before any download.
pub fn managed_path(root: &Path) -> PathBuf {
    storage::Storage::new(root).jar()
}

/// Prepares one verified managed JAR, then releases its lease before returning.
/// The downloader factory is called only when policy and local state require a download.
/// Dropping this future cancels network work and removes staging files.
pub async fn prepare(
    root: &Path,
    policy: JarPolicy,
    downloader: impl FnOnce() -> Result<Downloader>,
) -> Result<PathBuf> {
    prepare_with_progress(root, policy, downloader, &|_| Ok(())).await
}

/// Prepares the same managed pair while reporting its actual checks and installation stages.
pub async fn prepare_with_progress(
    root: &Path,
    policy: JarPolicy,
    downloader: impl FnOnce() -> Result<Downloader>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<PathBuf> {
    progress(Event::Checking)?;
    let storage = storage::Storage::new(root);
    let _lease = storage.lock()?;
    let installed = storage.inspect()?;
    match installed {
        Ok(metadata) if policy != JarPolicy::Refresh => {
            tracing::debug!(
                build = metadata.build_number,
                version = metadata.version,
                "Using verified authlib-injector JAR"
            );
            progress(Event::Ready)?;
            return Ok(storage.jar());
        }
        Err(reason) if policy == JarPolicy::Local => {
            return Err(reason).context(
                "authlib-injector requires repair, but the local JAR policy forbids downloading",
            );
        }
        Err(reason) => tracing::info!(reason = %reason, "Repairing managed authlib-injector JAR"),
        Ok(_) => tracing::info!("Refreshing managed authlib-injector JAR"),
    }
    let downloader = downloader()?;
    progress(Event::Resolving)?;
    let download_progress =
        |event: kako_mc_lib::download::Event<'_>| progress(Event::Download(event));
    let release = authlib_injector::latest_with_progress(&downloader, &download_progress).await?;
    tracing::info!(
        build = release.build_number(),
        version = release.version(),
        "Downloading authlib-injector"
    );
    let staged = storage.staging()?;
    let jar = staged.path().join("authlib-injector.jar");
    release
        .download_to_with_progress(&downloader, &jar, &download_progress)
        .await?;
    progress(Event::Installing)?;
    storage.publish(&jar, &metadata::Metadata::from_release(&release))?;
    progress(Event::Ready)?;
    Ok(storage.jar())
}

/// Repairable installed-state failures, distinct from filesystem and format incompatibility errors.
#[derive(Debug, thiserror::Error)]
enum Unavailable {
    #[error("managed authlib-injector JAR is missing")]
    MissingJar,
    #[error("managed authlib-injector metadata is missing")]
    MissingMetadata,
    #[error("managed authlib-injector metadata is corrupt")]
    InvalidMetadata(#[source] anyhow::Error),
    #[error("managed authlib-injector JAR does not match its metadata")]
    Checksum(#[source] anyhow::Error),
}
