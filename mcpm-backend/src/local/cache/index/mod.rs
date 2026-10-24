//! Sole owner of the optional SQLite imohash index, schema and recovery leases.

use super::{
    hash::{Blake3Hash, ImoHash},
    storage,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, params};
use std::{
    fs::{self, File, OpenOptions},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    time::Duration,
};

/// Fixes both SQL schema and the shared imohash-1.0 sampling parameters.
const VERSION: i64 = 1;

pub(super) fn path(root: &Path) -> PathBuf {
    root.join(".mcpm/index.sqlite3")
}

struct Database {
    connection: Connection,
    _lease: File,
}
impl Deref for Database {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.connection
    }
}
impl DerefMut for Database {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

fn lease(root: &Path, write: bool, exclusive: bool) -> Result<File> {
    let path = root.join(".mcpm/index.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(write)
        .create(write)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("failed to open quick-index lease {path:?}"))?;
    let result = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    result.with_context(|| format!("quick-index recovery lease is busy: {path:?}"))?;
    Ok(file)
}

fn connect(root: &Path, write: bool) -> Result<Database> {
    let lease = lease(root, write, false)?;
    Ok(Database {
        connection: open(root, write)?,
        _lease: lease,
    })
}

fn open(root: &Path, write: bool) -> Result<Connection> {
    let path = path(root);
    let exists = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file(),
                "expected a regular quick index: {path:?}"
            );
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect quick index {path:?}"));
        }
    };
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(&path, flags)
        .with_context(|| format!("failed to open quick index {path:?}"))?;
    connection.busy_timeout(Duration::ZERO)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if write && !exists && version == 0 {
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE quick_lookup (
                 imohash BLOB NOT NULL CHECK(length(imohash) = 16),
                 blake3 BLOB NOT NULL CHECK(length(blake3) = 32),
                 PRIMARY KEY (imohash, blake3)
             ) WITHOUT ROWID;
             CREATE INDEX quick_lookup_by_blake3 ON quick_lookup(blake3);",
        )?;
        connection.pragma_update(None, "user_version", VERSION)?;
        connection.execute_batch("COMMIT;")?;
    } else {
        ensure!(
            version == VERSION,
            "unsupported quick-index version {version}; expected {VERSION}"
        );
    }
    Ok(connection)
}

/// Opens existing state read-only. Missing, invalid or busy indexes are reported
/// to lookup, which falls back to full hashing without repairing derived state.
pub(super) fn candidates(root: &Path, quick: &ImoHash) -> Result<Vec<Blake3Hash>> {
    let database = connect(root, false)?;
    let mut statement =
        database.prepare("SELECT blake3 FROM quick_lookup WHERE imohash = ?1 ORDER BY blake3")?;
    let rows = statement.query_map([quick.as_bytes().as_slice()], |row| {
        row.get::<_, Vec<u8>>(0)
    })?;
    let mut keys = Vec::new();
    for row in rows {
        let bytes: [u8; 32] = row?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid BLAKE3 length in quick index"))?;
        keys.push(Blake3Hash::new(bytes));
    }
    Ok(keys)
}

fn insert(root: &Path, quick: &ImoHash, key: &Blake3Hash) -> Result<()> {
    connect(root, true)?.execute(
        "INSERT OR IGNORE INTO quick_lookup(imohash, blake3) VALUES (?1, ?2)",
        params![quick.as_bytes().as_slice(), key.as_bytes().as_slice()],
    )?;
    Ok(())
}

pub(super) fn record(root: &Path, quick: &ImoHash, key: &Blake3Hash) {
    if let Err(error) = insert(root, quick, key) {
        tracing::debug!(%key, error = %format!("{error:#}"), "Skipping optional quick-index update");
    }
}

fn remove(root: &Path, key: &Blake3Hash) -> Result<()> {
    // A removal never creates a missing index or attempts to adopt a damaged one.
    if !path(root)
        .try_exists()
        .context("failed to inspect quick index for removal")?
    {
        return Ok(());
    }
    connect(root, true)?.execute(
        "DELETE FROM quick_lookup WHERE blake3 = ?1",
        [key.as_bytes().as_slice()],
    )?;
    Ok(())
}

pub(super) fn forget(root: &Path, key: &Blake3Hash) {
    if let Err(error) = remove(root, key) {
        tracing::debug!(%key, error = %format!("{error:#}"), "Skipping optional quick-index removal");
    }
}

/// Exclusive leases prevent replacing a database still used by cooperating WAL
/// connections. Recovery affects only derived state; content remains authoritative.
pub(super) fn replace(root: &Path, entries: &[(ImoHash, Blake3Hash)]) -> Result<()> {
    let _lease = lease(root, true, true)?;
    for suffix in ["", "-wal", "-shm"] {
        storage::remove_if_exists(&root.join(format!(".mcpm/index.sqlite3{suffix}")))?;
    }
    let mut connection = open(root, true)?;
    let transaction = connection.transaction()?;
    {
        let mut statement = transaction
            .prepare("INSERT OR IGNORE INTO quick_lookup(imohash, blake3) VALUES (?1, ?2)")?;
        for (quick, key) in entries {
            statement.execute(params![
                quick.as_bytes().as_slice(),
                key.as_bytes().as_slice()
            ])?;
        }
    }
    transaction
        .commit()
        .context("failed to commit rebuilt quick index")
}

#[cfg(test)]
mod tests;
