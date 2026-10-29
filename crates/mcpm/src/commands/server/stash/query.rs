//! Shared read observations for stash commands; storage codecs and rendering remain with their owners.

use anyhow::Result;
use mcpm_backend::local::mc::server::stash::{Stash, StashStore};

use crate::{diagnostics, display::stash::Entry};

pub(super) fn observed_head(
    store: &StashStore<'_>,
    failures: &mut usize,
) -> Option<Option<uuid::Uuid>> {
    match store.head() {
        Ok(head) => Some(head.map(|stash| stash.id())),
        Err(error) => {
            report(&error, failures);
            None
        }
    }
}

pub(super) fn entry(
    store: &StashStore<'_>,
    stash: Stash,
    head: Option<Option<uuid::Uuid>>,
    manifest: bool,
) -> Result<Entry> {
    Ok(Entry {
        head: head.map(|head| head == Some(stash.id())),
        backup: store.backup_path(&stash),
        manifest: if manifest {
            Some(store.manifest(&stash)?)
        } else {
            None
        },
        stash,
    })
}

pub(super) fn report(error: &anyhow::Error, failures: &mut usize) {
    diagnostics::error(error);
    *failures += 1;
}
