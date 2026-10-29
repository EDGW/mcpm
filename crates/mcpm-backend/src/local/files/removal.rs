//! Recursive tree removal that temporarily unlocks directories while preserving linked files.

use std::{fs, path::Path};

use anyhow::{Context, Result};

use super::DirectoryProtection;

/// Restores surviving directory permissions on failure; never changes file ACLs or modes.
pub(crate) fn remove_protected_tree(path: &Path) -> Result<()> {
    let mut guards = Vec::new();
    unlock_directories(path, &mut guards)?;
    fs::remove_dir_all(path)
        .with_context(|| format!("failed to remove directory tree {path:?}"))?;
    for guard in guards {
        guard.persist();
    }
    Ok(())
}

fn unlock_directories(path: &Path, guards: &mut Vec<DirectoryProtection>) -> Result<()> {
    guards.push(
        DirectoryProtection::allow_removal(path)
            .with_context(|| format!("failed to permit removal from directory {path:?}"))?,
    );
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to enumerate directory {path:?}"))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            unlock_directories(&entry.path(), guards)?;
        }
    }
    Ok(())
}
