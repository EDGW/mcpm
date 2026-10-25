//! Process-scoped stash leases, including a side-effect-free empty snapshot before initialization.

use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use super::index::{directory, regular_exists};

/// A retained file handle owns the OS lock; the persistent lock file is never removed.
pub(super) fn acquire(root: &Path, write: bool) -> Result<Option<File>> {
    if !directory(root, write)? {
        return Ok(None);
    }
    let path = root.join("lock");
    let exists = regular_exists(&path)?;
    let file = if write {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
    } else if exists {
        OpenOptions::new().read(true).open(&path)
    } else {
        // Writers publish nothing until this lock exists and has been acquired.
        // Return a permanently empty view, not a later scan of a concurrently initialized root.
        ensure!(
            !regular_exists(&root.join("HEAD"))?,
            "stash HEAD exists without a coordination lock"
        );
        if directory(&root.join("index"), false)? {
            ensure!(
                fs::read_dir(root.join("index"))?.next().is_none(),
                "stash index exists without a coordination lock"
            );
        }
        return Ok(None);
    };
    let file = match file {
        Ok(file) => file,
        Err(error) if !write && error.kind() == io::ErrorKind::NotFound => {
            anyhow::bail!("stash coordination lock disappeared: {path:?}");
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to open stash lock {path:?}"));
        }
    };
    if write {
        file.try_lock()
    } else {
        file.try_lock_shared()
    }
    .with_context(|| format!("stash is busy or cannot be locked: {path:?}"))?;
    Ok(Some(file))
}
