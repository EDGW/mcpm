//! Server-local authlib-injector JAR ownership and agent identity, independent of TOML parsing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::download::{Downloader, authlib_injector};
use crate::local::java::jar;

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
    Download(crate::download::Event<'a>),
    Installing,
    Ready,
}

/// The sole managed JAR location, also used for argument preflight before any download.
pub(crate) fn managed_path(root: &Path) -> PathBuf {
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
    let download_progress = |event: crate::download::Event<'_>| progress(Event::Download(event));
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

/// Resolves a user-owned file without inspecting managed metadata or computing its digest.
pub(crate) fn manual(root: &Path, value: &str) -> Result<PathBuf> {
    let path = root.join(value);
    let file = std::fs::File::open(&path)
        .with_context(|| format!("failed to open manual authlib-injector JAR {path:?}"))?;
    ensure!(
        file.metadata()?.is_file(),
        "manual authlib-injector JAR is not a file: {path:?}"
    );
    Ok(path)
}

/// Extracts the Java agent's path and opaque options without interpreting other agents.
pub(crate) fn agent(argument: &str) -> Option<(&str, &str)> {
    argument.strip_prefix("-javaagent:")?.split_once('=')
}

/// JAR metadata is identity evidence; missing or unreadable evidence is not a guessed match.
pub(crate) fn is_agent(path: &Path) -> bool {
    jar::manifest(path).ok().is_some_and(|manifest| {
        manifest
            .get("premain-class")
            .is_some_and(|value| value == "moe.yushi.authlibinjector.Premain")
    })
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
