//! Execution-host management protection; stash owns path recognition and cache stays generic.

use anyhow::{Context, Result, bail};
use mcpm_lib::local::{cache::mods::ModsCacheSelection, mc::server::stash};
use std::path::{Path, PathBuf};

/// Checks all write destinations before any mutation, including empty write batches.
pub fn check_protection(
    selection: ModsCacheSelection,
    writes: bool,
    destinations: &[PathBuf],
    bypass: bool,
) -> Result<ModsCacheSelection> {
    if bypass {
        return Ok(selection);
    }
    let path = selection.path()?;
    let owner = stash::cache_owner(&path)?;
    if let Some(server) = owner.as_ref() {
        if writes {
            refuse(&path, server.clone())?;
        }
    }
    for destination in destinations {
        if let Some(server) = stash::cache_owner(destination)? {
            refuse(destination, server)?;
        }
    }
    Ok(if owner.is_some() {
        ModsCacheSelection::Directory { path }
    } else {
        selection
    })
}

fn refuse(path: &Path, server: PathBuf) -> Result<()> {
    bail!(ProtectedStashCache {
        path: path.into(),
        server
    });
}

#[derive(Debug, thiserror::Error)]
#[error(
    "refusing to modify server stash cache at {path:?} (server: {server:?}). Do not manage server stash caches manually. Stashes depend on their contents; changes may make stashes unusable. Use --no-protect-stash to bypass this protection."
)]
struct ProtectedStashCache {
    path: PathBuf,
    server: PathBuf,
}

pub(super) fn open(
    input: super::CacheInput,
    writes: bool,
    destinations: &[PathBuf],
) -> Result<mcpm_lib::local::cache::mods::ModsCacheDir> {
    check_protection(
        input.selection,
        writes,
        destinations,
        input.no_protect_stash,
    )?
    .open()
    .context("failed to open cache")
}
