//! Server Mod snapshots: serialized business operations over backup, with one HEAD comparison baseline.

use std::{collections::HashSet, fs, num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result, ensure};
use uuid::Uuid;

use super::MinecraftServer;
use crate::local::{
    cache::{CacheLinkPreference, mods::ModsCacheDir},
    mcmod::backup::{self, Event, Manifest},
};

mod create;
pub use create::CreateCleanupError;
mod index;
pub use index::Stash;
mod lock;

#[cfg(test)]
mod tests;

/// A server-scoped operation session; its lease is held until the session is dropped.
///
/// # Implementations
/// - [`Self::read`]: session leases, snapshot queries, switching, renaming and deletion (`mod.rs`).
/// - [`Self::create`]: snapshot creation and publication failure handling (`create.rs`).
pub struct StashStore<'a> {
    server: &'a MinecraftServer,
    index: index::Index,
    writable: bool,
    _lease: Option<fs::File>,
}

impl<'a> StashStore<'a> {
    /// Reads without initializing directories or opening a content cache.
    pub fn read(server: &'a MinecraftServer) -> Result<Self> {
        Self::open(server, false)
    }

    /// Acquires an exclusive lease before inspecting or modifying stash state.
    pub fn write(server: &'a MinecraftServer) -> Result<Self> {
        Self::open(server, true)
    }

    fn open(server: &'a MinecraftServer, writable: bool) -> Result<Self> {
        let root = fs::canonicalize(server.path()).context("failed to resolve server directory")?;
        index::directory(&root.join(".mcpm"), false)?;
        let root = root.join(".mcpm/stash");
        let lease = lock::acquire(&root, writable)?;
        let available = lease.is_some();
        Ok(Self {
            server,
            index: index::Index::new(root, available),
            writable,
            _lease: lease,
        })
    }

    /// Keeps individual corrupt records as errors so callers can show the remaining inventory.
    pub fn list(&self) -> Result<Vec<Result<Stash>>> {
        self.index.list()
    }

    pub fn resolve(&self, selector: &Selector) -> Result<Stash> {
        match selector {
            Selector::Id(id) => self.index.read(*id),
            Selector::Name(name) => {
                let mut found = None;
                for stash in self.list()? {
                    let stash = stash?;
                    if stash.name() == name {
                        ensure!(found.is_none(), "multiple stashes have name {name:?}");
                        found = Some(stash);
                    }
                }
                found.with_context(|| format!("stash name does not exist: {name:?}"))
            }
        }
    }

    /// Validates the HEAD reference and its baseline manifest, but not backup file contents.
    pub fn head(&self) -> Result<Option<Stash>> {
        self.index
            .head()?
            .map(|id| {
                let stash = self
                    .index
                    .read(id)
                    .context("HEAD references an unavailable stash")?;
                self.manifest(&stash)
                    .context("HEAD references an invalid backup manifest")?;
                Ok(stash)
            })
            .transpose()
    }

    pub fn backup_path(&self, stash: &Stash) -> PathBuf {
        self.index
            .root()
            .parent()
            .unwrap()
            .join("data")
            .join(stash.backup_id().to_string())
    }

    pub fn manifest(&self, stash: &Stash) -> Result<Manifest> {
        let backup = self.backup_path(stash);
        ensure!(
            index::directory(backup.parent().unwrap(), false)?,
            "backup data directory does not exist"
        );
        ensure!(
            index::directory(&backup, false)?,
            "stash backup does not exist: {backup:?}"
        );
        Manifest::read(&backup.join(backup::MANIFEST_NAME))
    }

    /// Compares all current files against HEAD without verifying saved backup data.
    pub fn status(
        &self,
        parallelism: NonZeroUsize,
        emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<Status> {
        let head = self.head()?;
        let current = backup::checksum(&self.mods_path(), parallelism, emit)?;
        let comparison = head
            .as_ref()
            .map(|stash| {
                self.manifest(stash)
                    .map(|manifest| current.compare_manifest(&manifest))
            })
            .transpose()?;
        Ok(Status { head, comparison })
    }

    /// Fully verifies the target even for force and no-op switches; force only permits dirty replacement.
    pub fn switch(
        &mut self,
        selector: &Selector,
        force: bool,
        parallelism: NonZeroUsize,
        mut emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<SwitchOutcome> {
        self.require_write()?;
        let stash = self.resolve(selector)?;
        self.verify_stash(&stash, parallelism, &mut emit)
            .context("target stash failed verification; switch refused")?;
        let status = self.status(parallelism, &mut emit)?;
        ensure!(
            force || status.state() == WorkspaceState::Clean,
            "workspace has unsaved changes or no HEAD; create a stash first or use --force to discard its contents"
        );
        if status.state() == WorkspaceState::Clean
            && status
                .head
                .as_ref()
                .is_some_and(|head| head.id() == stash.id())
        {
            return Ok(SwitchOutcome {
                stash,
                changed: false,
            });
        }
        backup::apply_with_cache(
            &self.backup_path(&stash),
            &self.mods_path(),
            || self.open_cache(),
            backup::ApplyOptions {
                mode: CacheLinkPreference::Copy,
                parallelism,
                force: true,
            },
            &mut emit,
        )?;
        self.index
            .set_head(&stash)
            .context("Mod files were switched, but updating HEAD failed")?;
        Ok(SwitchOutcome {
            stash,
            changed: true,
        })
    }

    pub fn rename(&mut self, selector: &Selector, name: String) -> Result<Stash> {
        self.require_write()?;
        index::validate_name(&name)?;
        let mut stash = self.resolve(selector)?;
        self.require_unique_name(&name, Some(stash.id()))?;
        if stash.name() != name {
            stash.rename(name)?;
            self.index.publish(&stash, true)?;
        }
        Ok(stash)
    }

    /// Preflights the entire batch's HEAD protection before deleting any backup.
    pub fn delete(&mut self, selectors: &[Selector]) -> Result<Vec<Deletion>> {
        self.require_write()?;
        let head = self.head()?;
        let mut seen = HashSet::new();
        let selected: Vec<_> = selectors
            .iter()
            .map(|selector| (selector.clone(), self.resolve(selector)))
            .filter(|(_, result)| {
                result
                    .as_ref()
                    .map_or(true, |stash| seen.insert(stash.id()))
            })
            .collect();
        if let Some(head) = head {
            ensure!(
                !selected
                    .iter()
                    .any(|(_, result)| result.as_ref().is_ok_and(|stash| stash.id() == head.id())),
                "cannot delete HEAD stash {}; switch to another stash first; no stashes were deleted",
                head.id()
            );
        }
        Ok(selected
            .into_iter()
            .map(|(selector, result)| {
                let result = (|| {
                    let stash = result?;
                    // An invalid shared reference must never let one deletion destroy another stash.
                    for other in self.list()? {
                        let other = other
                            .context("cannot establish exclusive ownership of stash backup")?;
                        ensure!(
                            other.id() == stash.id() || other.backup_id() != stash.backup_id(),
                            "backup {} is also referenced by stash {}; refusing deletion",
                            stash.backup_id(),
                            other.id()
                        );
                    }
                    let path = self.backup_path(&stash);
                    if index::directory(path.parent().unwrap(), false)?
                        && index::directory(&path, false)?
                    {
                        backup::delete(&path).with_context(|| {
                            format!("failed to delete backup for stash {}", stash.id())
                        })?;
                    }
                    self.index.remove(stash.id())?;
                    Ok(stash)
                })()
                .with_context(|| format!("failed to delete stash {selector:?}"));
                Deletion { selector, result }
            })
            .collect())
    }

    pub fn verify(
        &self,
        selector: &Selector,
        parallelism: NonZeroUsize,
        emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<Stash> {
        let stash = self.resolve(selector)?;
        self.verify_stash(&stash, parallelism, emit)?;
        Ok(stash)
    }

    fn verify_stash(
        &self,
        stash: &Stash,
        parallelism: NonZeroUsize,
        emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<()> {
        self.manifest(stash)?;
        backup::verify_with_cache(
            &self.backup_path(stash),
            || self.open_cache(),
            parallelism,
            emit,
        )?;
        Ok(())
    }

    /// Opens existing server-local content without initializing or falling back to a global cache.
    fn open_cache(&self) -> Result<ModsCacheDir> {
        ModsCacheDir::new(self.server.runtime().cache.stash.selection().path()?)
            .context("failed to open server stash content cache")
    }

    fn require_unique_name(&self, name: &str, except: Option<Uuid>) -> Result<()> {
        for stash in self.list()? {
            let stash = stash?;
            ensure!(
                Some(stash.id()) == except || stash.name() != name,
                "stash name already exists: {name:?}"
            );
        }
        Ok(())
    }

    fn require_write(&self) -> Result<()> {
        ensure!(self.writable, "stash session is read-only");
        Ok(())
    }

    fn mods_path(&self) -> PathBuf {
        self.server.path().join("mods")
    }
}

/// Explicit selectors never infer whether a string is a UUID or a name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Selector {
    Id(Uuid),
    Name(String),
}

/// A batch deletion retains its input identity even when lookup or removal fails.
pub struct Deletion {
    pub selector: Selector,
    pub result: Result<Stash>,
}

/// A computed workspace comparison; untracked workspaces have no expected manifest.
pub struct Status {
    pub head: Option<Stash>,
    pub comparison: Option<backup::Comparison>,
}

impl Status {
    pub fn state(&self) -> WorkspaceState {
        match &self.comparison {
            None => WorkspaceState::Untracked,
            Some(comparison) if comparison.matches() => WorkspaceState::Clean,
            Some(_) => WorkspaceState::Modified,
        }
    }
}

/// Operational errors are returned separately rather than masquerading as modifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceState {
    Clean,
    Modified,
    Untracked,
}

/// A verified switch can succeed without replacing an already matching workspace.
pub struct SwitchOutcome {
    pub stash: Stash,
    pub changed: bool,
}
