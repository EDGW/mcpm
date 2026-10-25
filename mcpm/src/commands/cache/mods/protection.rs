//! Preflight stash cache protection for explicit cache management operations.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use mcpm_backend::local::{cache::mods::ModsCacheSelection, mc::server::stash};

use super::Command;

pub(super) fn check(
    selection: ModsCacheSelection,
    command: &Command,
    bypass: bool,
) -> Result<ModsCacheSelection> {
    if bypass {
        return Ok(selection);
    }
    let path = selection.path()?;
    if let Some(server) = stash::cache_owner(&path)? {
        if command.writes_cache() {
            refuse_write(&path, server)?;
        }
        // Even read commands normally initialize the default cache. Opening an
        // existing directory instead keeps a default-path alias truly read-only.
        check_destinations(command)?;
        return Ok(ModsCacheSelection::Directory { path });
    }
    check_destinations(command)?;
    Ok(selection)
}

fn check_destinations(command: &Command) -> Result<()> {
    if let Command::Export(args) = command {
        for path in args.destinations() {
            if let Some(server) = stash::cache_owner(path)? {
                refuse_write(path, server)?;
            }
        }
    }
    Ok(())
}

fn refuse_write(path: &Path, server: PathBuf) -> Result<()> {
    crate::display::cache::stash_protection_notice()?;
    bail!(ProtectedStashCache {
        path: path.to_owned(),
        server
    });
}

#[derive(Debug, thiserror::Error)]
#[error("refusing to modify server stash cache at {path:?} (server: {server:?})")]
struct ProtectedStashCache {
    path: PathBuf,
    server: PathBuf,
}
