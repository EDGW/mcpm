//! Snapshot creation and its dedicated dual-failure error; publication and cleanup retain separate causes.

use std::{num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result, ensure};

use super::{Stash, StashStore, WorkspaceState, index};
use crate::local::{
    cache::{CacheLinkPreference, mods::ModsCacheDir},
    mcmod::backup::{self, Event},
};

impl StashStore<'_> {
    /// Publishes a new snapshot and then advances HEAD; clean workspaces are not snapshotted twice.
    pub fn create(
        &mut self,
        name: String,
        parallelism: NonZeroUsize,
        mut emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<Stash> {
        self.require_write()?;
        let stash = Stash::new(name)?;
        self.require_unique_name(stash.name(), None)?;
        ensure!(
            self.status(parallelism, &mut emit)?.state() != WorkspaceState::Clean,
            "workspace is unchanged; creating another stash is unnecessary"
        );
        let data = self.backup_path(&stash).parent().unwrap().to_owned();
        index::directory(&data, true)?;
        let runtime_cache = &self.server.runtime().cache.stash;
        ModsCacheDir::initialize(runtime_cache.selection().path()?)?;
        let outcome = backup::create_with_cache(
            &self.mods_path(),
            &self.backup_path(&stash),
            runtime_cache.open()?,
            backup::CreateOptions {
                mode: CacheLinkPreference::PreferHardLink,
                parallelism,
            },
            &mut emit,
        )?;
        if let Err(error) = self.index.publish(&stash, false) {
            return match backup::delete(outcome.destination()) {
                Ok(()) => {
                    Err(error).context("failed to publish stash index; new backup was removed")
                }
                Err(cleanup) => Err(CreateCleanupError {
                    backup: outcome.destination().to_owned(),
                    publication: error,
                    cleanup: cleanup.context("failed to clean up the unpublished stash backup"),
                }
                .into()),
            };
        }
        self.index
            .set_head(&stash)
            .context("stash was saved, but updating HEAD failed")?;
        Ok(stash)
    }
}

/// Creation failed during index publication and cleanup; both original error chains are retained.
#[derive(Debug, thiserror::Error)]
#[error("failed to publish stash; cleanup also failed; unreferenced backup remains at {backup:?}")]
pub struct CreateCleanupError {
    backup: PathBuf,
    #[source]
    publication: anyhow::Error,
    cleanup: anyhow::Error,
}

impl CreateCleanupError {
    /// The backup retained after cleanup failed.
    pub fn backup(&self) -> &std::path::Path {
        &self.backup
    }

    /// The primary source chain describes publication; this independent chain describes cleanup.
    pub fn cleanup(&self) -> &anyhow::Error {
        &self.cleanup
    }
}
