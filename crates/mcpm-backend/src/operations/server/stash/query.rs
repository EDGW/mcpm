//! Non-streaming host operations capture results under one lease and release it before rendering.

use anyhow::Result;
use mcpm_exec::operation::Operation;
use mcpm_lib::local::mc::server::stash::StashStore;

use super::*;

/// List indices without loading entry manifests.
pub struct List;
impl Operation for List {
    const ID: &'static str = "server.stash.list";
    type Input = ListInput;
    type Output = ListOutput;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || {
            let server = crate::workspace::inspect_local(input.path)?;
            let store = StashStore::read(&server)?;
            let head = store
                .head()
                .map(|head| head.map(|stash| stash.id()))
                .map_err(Into::into);
            let entries = store
                .list()?
                .into_iter()
                .map(|result| {
                    result
                        .map(|stash| EntrySnapshot {
                            backup_path: store.backup_path(&stash).display().to_string(),
                            stash: (&stash).into(),
                        })
                        .map_err(Into::into)
                })
                .collect();
            Ok(ListOutput { head, entries })
        })
        .await?
    }
}

/// Read selected indices, loading manifests only when requested by the caller.
pub struct Info;
impl Operation for Info {
    const ID: &'static str = "server.stash.info";
    type Input = InfoInput;
    type Output = InfoOutput;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || {
            let server = crate::workspace::inspect_local(input.path)?;
            let store = StashStore::read(&server)?;
            let head = store
                .head()
                .map(|head| head.map(|stash| stash.id()))
                .map_err(Into::into);
            let entries = super::resolve(&store, input.selectors)
                .into_iter()
                .map(|(_, result)| {
                    result
                        .and_then(|stash| {
                            Ok(DetailedEntry {
                                backup_path: store.backup_path(&stash).display().to_string(),
                                manifest: if input.include_manifest {
                                    Some((&store.manifest(&stash)?).into())
                                } else {
                                    None
                                },
                                stash: (&stash).into(),
                            })
                        })
                        .map_err(Into::into)
                })
                .collect();
            Ok(InfoOutput { head, entries })
        })
        .await?
    }
}

/// Rename under an exclusive lease without changing HEAD.
pub struct Rename;
impl Operation for Rename {
    const ID: &'static str = "server.stash.rename";
    type Input = RenameInput;
    type Output = StashSnapshot;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || {
            let server = crate::workspace::inspect_local(input.path)?;
            let mut store = StashStore::write(&server)?;
            let stash = store.rename(&(&input.selector).into(), input.new_name)?;
            Ok((&stash).into())
        })
        .await?
    }
}

/// Delete one batch only after the library has preflighted HEAD protection for all selectors.
pub struct Delete;
impl Operation for Delete {
    const ID: &'static str = "server.stash.delete";
    type Input = DeleteInput;
    type Output = Vec<DeletionResult>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        tokio::task::spawn_blocking(move || {
            let server = crate::workspace::inspect_local(input.path)?;
            let mut store = StashStore::write(&server)?;
            let selectors: Vec<_> = input.selectors.iter().map(Into::into).collect();
            Ok(store
                .delete(&selectors)?
                .into_iter()
                .map(|deletion| DeletionResult {
                    selector: deletion.selector.into(),
                    result: deletion
                        .result
                        .map(|stash| (&stash).into())
                        .map_err(Into::into),
                })
                .collect())
        })
        .await?
    }
}
