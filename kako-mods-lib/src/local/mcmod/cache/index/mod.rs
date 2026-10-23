//! Sole owner of the rebuildable SQLite imohash-to-BLAKE3 index and its schema.

use std::{
    fs::{File, OpenOptions},
    ops::{Deref, DerefMut},
    path::Path,
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, params};

use super::{CacheError, CacheId, error::IoContext};

pub(super) mod sample;

/// Version fixes both schema and imohash algorithm/sampling parameters.
const INDEX_VERSION: i64 = 1;

fn path(root: &Path) -> std::path::PathBuf {
    root.join(".mcpm/index.sqlite3")
}

/// A nonblocking index-only lease prevents recovery from unlinking a live WAL database.
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

fn lease(root: &Path, write: bool, exclusive: bool) -> Result<File, CacheError> {
    let path = root.join(".mcpm/index.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(write)
        .create(write)
        .truncate(false)
        .open(&path)
        .at("open quick-index recovery lease", &path)?;
    let result = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    result.map_err(|error| CacheError::IndexBusy(error.to_string()))?;
    Ok(file)
}

fn connect(root: &Path, write: bool) -> Result<Database, CacheError> {
    let lease = lease(root, write, false)?;
    Ok(Database {
        connection: open(root, write)?,
        _lease: lease,
    })
}

fn open(root: &Path, write: bool) -> Result<Connection, CacheError> {
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(path(root), flags)?;
    // Cache writes may be dropped under contention; never wait for a busy writer.
    connection.busy_timeout(Duration::ZERO)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if write && version == 0 {
        // Initialization is a single SQLite transaction. A competing initializer
        // can fail busy; the cache operation itself remains usable without an index.
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS quick_lookup (
                 imohash BLOB NOT NULL CHECK(length(imohash) = 16),
                 blake3 BLOB NOT NULL CHECK(length(blake3) = 32),
                 PRIMARY KEY (imohash, blake3)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS quick_lookup_by_blake3 ON quick_lookup(blake3);
             ",
        )?;
        connection.pragma_update(None, "user_version", INDEX_VERSION)?;
        connection.execute_batch("COMMIT;")?;
    } else if version != INDEX_VERSION {
        return Err(CacheError::UnsupportedIndexVersion(version));
    }
    Ok(connection)
}

/// Index failure is a performance miss, never proof of absent content.
pub(super) fn candidates(root: &Path, hash: &[u8; 16]) -> Vec<CacheId> {
    let result = (|| {
        let connection = connect(root, false)?;
        let mut statement =
            connection.prepare("SELECT blake3 FROM quick_lookup WHERE imohash = ?1")?;
        let rows = statement.query_map([hash.as_slice()], |row| row.get::<_, Vec<u8>>(0))?;
        let mut result = Vec::new();
        for row in rows {
            if let Ok(bytes) = <[u8; 32]>::try_from(row?) {
                result.push(CacheId(bytes));
            }
        }
        Ok::<_, CacheError>(result)
    })();
    result.unwrap_or_else(|error| {
        tracing::debug!(%error, "Quick index unavailable; using full BLAKE3 lookup");
        Vec::new()
    })
}

pub(super) fn insert(root: &Path, hash: &[u8; 16], id: CacheId) -> Result<(), CacheError> {
    connect(root, true)?.execute(
        "INSERT OR IGNORE INTO quick_lookup(imohash, blake3) VALUES (?1, ?2)",
        params![hash.as_slice(), id.0.as_slice()],
    )?;
    Ok(())
}

pub(super) fn remove(root: &Path, id: CacheId) -> Result<(), CacheError> {
    // Do not create an index merely to delete from it.
    if !path(root).is_file() {
        return Ok(());
    }
    connect(root, true)?.execute(
        "DELETE FROM quick_lookup WHERE blake3 = ?1",
        [id.0.as_slice()],
    )?;
    Ok(())
}

/// Uses SQLite's own transaction, not a cache-wide application lock.
/// The derived index may be incomplete after concurrent filesystem changes.
pub(super) fn replace(root: &Path, entries: &[([u8; 16], CacheId)]) -> Result<(), CacheError> {
    let _recovery = lease(root, true, true)?;
    // Recovery may discard this entire derived database. Shared leases ensure
    // no cooperating connection is still attached to the old WAL/shm files.
    for suffix in ["", "-wal", "-shm"] {
        let file = root.join(format!(".mcpm/index.sqlite3{suffix}"));
        super::storage::remove_if_exists(&file)?;
    }
    let mut connection = open(root, true)?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM quick_lookup", [])?;
    {
        let mut statement = transaction
            .prepare("INSERT OR IGNORE INTO quick_lookup(imohash, blake3) VALUES (?1, ?2)")?;
        for (hash, id) in entries {
            statement.execute(params![hash.as_slice(), id.0.as_slice()])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_busy_writes_are_optional_and_recovery_does_not_unlink_live_database() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".mcpm")).unwrap();
        let hash = [0; 16];
        let id = CacheId([1; 32]);
        insert(root.path(), &hash, id).unwrap();
        let connection = connect(root.path(), true).unwrap();
        connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(insert(root.path(), &hash, CacheId([2; 32])).is_err());
        assert_eq!(candidates(root.path(), &hash), vec![id]);
        assert!(matches!(
            replace(root.path(), &[]),
            Err(CacheError::IndexBusy(_))
        ));
        connection.execute_batch("ROLLBACK").unwrap();
        drop(connection);
        replace(root.path(), &[(hash, id)]).unwrap();
        assert_eq!(candidates(root.path(), &hash), vec![id]);
    }

    use std::fs;
}
