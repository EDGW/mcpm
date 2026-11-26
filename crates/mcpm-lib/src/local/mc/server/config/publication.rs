//! Pure 3-file atomic transaction engine (`.mcpm/server.json`, `launch.toml`, `conf.toml`)
//! with durable preimages, rollback recovery, and the `.mcpm/server.lock` coordination lock.

use super::{ImportedSetting, LaunchConfig};
use crate::local::mc::server::{MinecraftServer, MinecraftServerMetadata, metadata};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Transactional 3-file publication plan for `.mcpm/server.json`, `launch.toml`, and `conf.toml`.
pub struct Publication<'a> {
    pub identity: &'a MinecraftServerMetadata,
    pub launch: LaunchAction<'a>,
    pub conf: ConfAction<'a>,
    pub replace_existing: bool,
    pub recover_pending: bool,
}

/// Target transition for `launch.toml` within a [`Publication`].
pub enum LaunchAction<'a> {
    Keep,
    Write(&'a LaunchConfig),
    Remove,
}

/// Target transition for `conf.toml` within a [`Publication`].
pub enum ConfAction<'a> {
    Keep,
    Write(&'a str),
}

/// Actual filesystem outcome; suggestions are never silently applied to preserved conf.
#[derive(Debug)]
pub struct Published {
    pub server: MinecraftServer,
    pub created: Vec<PathBuf>,
    pub replaced: Vec<PathBuf>,
    pub preserved: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub imported: Vec<ImportedSetting>,
    pub entry_configured: bool,
}

/// Atomically publishes `.mcpm/server.json` together with the requested `launch.toml` and
/// `conf.toml` transitions while the caller holds an exclusive [`ServerLock`].
///
/// A v1 journal is removed only after all target files have been installed. When
/// `plan.recover_pending` is `true`, any interrupted publication is rolled back first;
/// otherwise a pending journal is rejected immediately.
pub fn commit(root: &Path, lock: &ServerLock, plan: Publication<'_>) -> Result<Published> {
    ensure!(
        root.is_dir(),
        "server root is not an existing directory: {root:?}"
    );
    lock.ensure_write_for(root, "configuration publication")?;
    let (identity, metadata_bytes) = metadata::encode(root, plan.identity)?;
    let launch_bytes = match plan.launch {
        LaunchAction::Write(launch) => Some(launch.encode()?.into_bytes()),
        LaunchAction::Keep | LaunchAction::Remove => None,
    };
    let server = MinecraftServer::registration_snapshot(root.to_owned(), identity)?;
    if plan.recover_pending {
        recover(root).context(
            "failed to recover interrupted configuration publication; no files were modified",
        )?;
    } else {
        ServerLock::require_clean(root)?;
    }
    let old_metadata = existing(&Target::Metadata.path(root))?;
    let (old_launch, launch_exists, preserved_launch) = match plan.launch {
        LaunchAction::Keep => {
            let exists = path_exists(&Target::Launch.path(root))?;
            (None, exists, exists)
        }
        LaunchAction::Write(_) | LaunchAction::Remove => {
            let old = existing(&Target::Launch.path(root))?;
            let exists = old.is_some();
            (old, exists, false)
        }
    };
    let (old_conf, preserved_conf) = match plan.conf {
        ConfAction::Keep => (None, path_exists(&Target::Conf.path(root))?),
        ConfAction::Write(_) => (existing(&Target::Conf.path(root))?, false),
    };
    ensure!(
        plan.replace_existing || (old_metadata.is_none() && !launch_exists),
        "server registration or launch.toml already exists; use --force to import again"
    );
    let mut journal = Journal {
        version: 1,
        files: vec![Change {
            target: Target::Metadata,
            before: old_metadata,
            after: Some(metadata_bytes),
        }],
    };
    match plan.launch {
        LaunchAction::Keep => {}
        LaunchAction::Write(_) => {
            journal.files.push(Change {
                target: Target::Launch,
                before: old_launch,
                after: launch_bytes,
            });
        }
        LaunchAction::Remove => {
            if old_launch.is_some() {
                journal.files.push(Change {
                    target: Target::Launch,
                    before: old_launch,
                    after: None,
                });
            }
        }
    }
    match plan.conf {
        ConfAction::Keep => {}
        ConfAction::Write(text) => {
            journal.files.push(Change {
                target: Target::Conf,
                before: old_conf,
                after: Some(text.as_bytes().to_vec()),
            });
        }
    }
    let journal_path = journal_path(root);
    let bytes =
        serde_json::to_vec(&journal).context("failed to encode configuration recovery journal")?;
    replace(&journal_path, &bytes).context("failed to prepare configuration recovery journal")?;
    let outcome = (|| -> Result<()> {
        for change in &journal.files {
            install(&change.target.path(root), change.after.as_deref())?;
        }
        fs::remove_file(&journal_path).context("failed to finish configuration publication")?;
        Ok(())
    })();
    if let Err(source) = outcome {
        if let Err(recovery) = recover(root) {
            return Err(PublicationRecoveryError {
                errors: crate::error::CombinedError::new(source, recovery),
                journal: journal_path,
            }
            .into());
        }
        return Err(source)
            .context("configuration publication failed; previous file contents were restored");
    }
    sync_directory(&root.join(".mcpm")).context("configuration files were committed, but synchronizing the commit marker failed; inspect the installed files before retrying")?;
    let mut result = Published {
        server,
        created: Vec::new(),
        replaced: Vec::new(),
        preserved: Vec::new(),
        removed: Vec::new(),
        imported: Vec::new(),
        entry_configured: matches!(plan.launch, LaunchAction::Write(_)),
    };
    for change in journal.files {
        let destination = if change.after.is_none() {
            &mut result.removed
        } else if change.before.is_some() {
            &mut result.replaced
        } else {
            &mut result.created
        };
        destination.push(change.target.path(root));
    }
    if preserved_launch {
        result.preserved.push(Target::Launch.path(root));
    }
    if preserved_conf {
        result.preserved.push(Target::Conf.path(root));
    }
    Ok(result)
}

/// Recovers a pending configuration publication journal while holding an exclusive
/// [`ServerLock`].
///
/// Returns `Ok(true)` if a pending journal existed and its preimages were restored, or
/// `Ok(false)` if no pending journal was present.
pub fn recover_if_pending(root: &Path, lock: &ServerLock) -> Result<bool> {
    lock.ensure_write_for(root, "configuration recovery")?;
    recover(root).context("failed to recover interrupted configuration publication")
}

/// Preserves both independent error chains if automatic rollback also fails.
#[derive(Debug, Error)]
#[error(
    "configuration publication and recovery failed; retry the interrupted operation to recover from {journal:?}"
)]
pub struct PublicationRecoveryError {
    #[source]
    pub errors: crate::error::CombinedError,
    pub journal: PathBuf,
}

/// An interrupted v1 configuration publication journal blocks reads and new commits until recovered.
#[derive(Debug, Error)]
#[error(
    "interrupted configuration publication requires explicit recovery through init --force or add-loader"
)]
pub struct PendingPublication {
    pub journal: PathBuf,
}

/// Short-lived coordination lock on `.mcpm/server.lock` serializing server configuration writes,
/// launch preparation/startup critical sections, and consistent reads.
pub struct ServerLock {
    _file: Option<File>,
    root: PathBuf,
    write: bool,
}

impl ServerLock {
    /// Acquires a shared read lock on `.mcpm/server.lock` with zero disk writes.
    ///
    /// If `.mcpm/server.lock` does not exist yet, neither `.mcpm/` nor the lock file is created;
    /// pending publication journals are still rejected.
    pub fn acquire_read(root: &Path) -> Result<Self> {
        let canonical_root = resolve_root(root)?;
        let lock_path = server_lock_path(&canonical_root);
        let file = match fs::symlink_metadata(&lock_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Ok(meta) => {
                ensure!(
                    meta.is_file(),
                    "server lock path is not a regular file: {lock_path:?}"
                );
                Some(open_existing_shared(&lock_path)?)
            }
            Err(error) => return Err(error).context("failed to inspect server lock"),
        };
        Self::require_clean(&canonical_root)?;
        Ok(Self {
            _file: file,
            root: canonical_root,
            write: false,
        })
    }

    /// Acquires a real OS shared lock on an existing `.mcpm/server.lock` without creating files
    /// or checking for pending journals. Fails if `.mcpm/server.lock` does not exist.
    pub fn acquire_existing_read(root: &Path) -> Result<Self> {
        let canonical_root = resolve_root(root)?;
        let lock_path = server_lock_path(&canonical_root);
        match fs::symlink_metadata(&lock_path) {
            Ok(meta) => ensure!(
                meta.is_file(),
                "server lock path is not a regular file: {lock_path:?}"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                bail!("server lock file does not exist: {lock_path:?}");
            }
            Err(error) => return Err(error).context("failed to inspect server lock"),
        }
        let file = open_existing_shared(&lock_path)?;
        Ok(Self {
            _file: Some(file),
            root: canonical_root,
            write: false,
        })
    }

    /// Acquires an exclusive write lock on `.mcpm/server.lock`, creating `.mcpm/` as needed.
    ///
    /// Does not reject pending journals so recovery workflows can hold the write lock.
    pub fn acquire_write(root: &Path) -> Result<Self> {
        let canonical_root = resolve_root(root)?;
        let mcpm_dir = canonical_root.join(".mcpm");
        match fs::create_dir(&mcpm_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).context("failed to create server metadata directory");
            }
        }
        let meta = fs::symlink_metadata(&mcpm_dir)
            .context("failed to inspect server metadata directory")?;
        ensure!(
            meta.is_dir(),
            "server metadata path is not a regular directory: {mcpm_dir:?}"
        );
        let lock_path = server_lock_path(&canonical_root);
        match fs::symlink_metadata(&lock_path) {
            Ok(meta) => ensure!(
                meta.is_file(),
                "server lock path is not a regular file: {lock_path:?}"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to inspect server lock"),
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(&lock_path)
            .with_context(|| format!("failed to open server lock {lock_path:?}"))?;
        file.try_lock()
            .context("server is busy or cannot be locked")?;
        Ok(Self {
            _file: Some(file),
            root: canonical_root,
            write: true,
        })
    }

    /// Returns the canonical lock file path `<root>/.mcpm/server.lock`.
    pub fn path(&self) -> PathBuf {
        server_lock_path(&self.root)
    }

    /// Rejects operations when a pending configuration publication journal exists.
    pub fn require_clean(root: &Path) -> Result<()> {
        let path = journal_path(root);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error)
                .with_context(|| format!("failed to inspect configuration journal {path:?}")),
            Ok(_) => Err(PendingPublication { journal: path }.into()),
        }
    }

    pub(crate) fn ensure_held_for(&self, root: &Path, action: &str) -> Result<()> {
        let canonical = fs::canonicalize(root)
            .with_context(|| format!("failed to resolve server root {root:?}"))?;
        ensure!(
            self.root == canonical,
            "{action} lock belongs to {:?}, not {:?}",
            self.root,
            canonical
        );
        Ok(())
    }

    pub(crate) fn ensure_write_for(&self, root: &Path, action: &str) -> Result<()> {
        ensure!(self.write, "{action} requires an exclusive server lock");
        self.ensure_held_for(root, action)
    }
}

impl Drop for ServerLock {
    fn drop(&mut self) {
        if let Some(file) = &self._file {
            // Closing one descriptor does not release a lock inherited by another process.
            if let Err(error) = file.unlock() {
                tracing::warn!(
                    path = %self.path().display(),
                    %error,
                    "failed to release server lock"
                );
            }
        }
    }
}

fn resolve_root(root: &Path) -> Result<PathBuf> {
    ensure!(
        root.is_dir(),
        "server root is not an existing directory: {root:?}"
    );
    fs::canonicalize(root).with_context(|| format!("failed to resolve server root {root:?}"))
}

fn server_lock_path(root: &Path) -> PathBuf {
    root.join(".mcpm/server.lock")
}

fn open_existing_shared(lock_path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(lock_path)
        .with_context(|| format!("failed to open server lock {lock_path:?}"))?;
    file.try_lock_shared()
        .context("server is busy or cannot be locked")?;
    Ok(file)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    files: Vec<Change>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    target: Target,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Target {
    Metadata,
    Launch,
    Conf,
}

impl Target {
    fn path(self, root: &Path) -> PathBuf {
        root.join(match self {
            Self::Metadata => ".mcpm/server.json",
            Self::Launch => "launch.toml",
            Self::Conf => "conf.toml",
        })
    }
}

fn recover(root: &Path) -> Result<bool> {
    let path = journal_path(root);
    let Some(bytes) = existing(&path)? else {
        return Ok(false);
    };
    let journal: Journal =
        serde_json::from_slice(&bytes).context("invalid configuration recovery journal")?;
    ensure!(
        journal.version == 1 && !journal.files.is_empty() && journal.files.len() <= 3,
        "unsupported configuration recovery journal"
    );
    for (index, change) in journal.files.iter().enumerate() {
        ensure!(
            !journal.files[..index]
                .iter()
                .any(|previous| previous.target == change.target),
            "duplicate recovery target"
        );
    }
    for change in journal.files.iter().rev() {
        install(&change.target.path(root), change.before.as_deref())
            .context("failed to restore configuration preimage")?;
    }
    fs::remove_file(path).context("failed to remove recovered configuration journal")?;
    sync_directory(&root.join(".mcpm"))?;
    Ok(true)
}

fn journal_path(root: &Path) -> PathBuf {
    root.join(".mcpm/launch-publication.json")
}

pub(super) fn existing(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file(),
                "configuration path is not a regular file: {path:?}"
            );
            Ok(Some(
                fs::read(path).with_context(|| format!("failed to read {path:?}"))?,
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {path:?}")),
    }
}

fn path_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {path:?}")),
    }
}

fn install(path: &Path, bytes: Option<&[u8]>) -> Result<()> {
    if let Some(bytes) = bytes {
        replace(path, bytes)
    } else {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("failed to remove {path:?}")),
        }
        sync_directory(
            path.parent()
                .context("configuration target has no parent")?,
        )
    }
}

fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration target has no parent")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {path:?}"))?;
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?
        .sync_all()
        .with_context(|| format!("failed to synchronize directory {path:?}"))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_mixed_generation_is_rejected_by_readers_and_restored_on_import_recovery() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".mcpm")).unwrap();
        let journal = Journal {
            version: 1,
            files: vec![
                Change {
                    target: Target::Metadata,
                    before: Some(b"old metadata".to_vec()),
                    after: Some(b"new metadata".to_vec()),
                },
                Change {
                    target: Target::Launch,
                    before: None,
                    after: Some(b"new launch".to_vec()),
                },
                Change {
                    target: Target::Conf,
                    before: Some(b"old conf".to_vec()),
                    after: Some(b"new conf".to_vec()),
                },
            ],
        };
        replace(
            &journal_path(root.path()),
            &serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        // Simulate interruption after two of the three files were replaced.
        install(&Target::Metadata.path(root.path()), Some(b"new metadata")).unwrap();
        install(&Target::Launch.path(root.path()), Some(b"new launch")).unwrap();
        install(&Target::Conf.path(root.path()), Some(b"old conf")).unwrap();
        assert!(ServerLock::acquire_read(root.path()).is_err());
        assert!(!server_lock_path(root.path()).exists());
        let lock = ServerLock::acquire_write(root.path()).unwrap();
        assert!(recover_if_pending(root.path(), &lock).unwrap());
        assert_eq!(
            fs::read(Target::Metadata.path(root.path())).unwrap(),
            b"old metadata"
        );
        assert_eq!(
            fs::read(Target::Conf.path(root.path())).unwrap(),
            b"old conf"
        );
        assert!(!Target::Launch.path(root.path()).exists());
        assert!(!journal_path(root.path()).exists());
        // Recovery is idempotent after every restored preimage.
        assert!(!recover_if_pending(root.path(), &lock).unwrap());
        assert!(!recover(root.path()).unwrap());
        drop(lock);

        let missing = root.path().join("does-not-exist");
        assert!(ServerLock::acquire_write(&missing).is_err());
        assert!(!missing.exists());

        let shared = ServerLock::acquire_read(root.path()).unwrap();
        assert!(recover_if_pending(root.path(), &shared).is_err());
        drop(shared);

        let other = tempfile::tempdir().unwrap();
        let other_lock = ServerLock::acquire_write(other.path()).unwrap();
        assert!(recover_if_pending(root.path(), &other_lock).is_err());
    }

    #[test]
    fn acquire_read_performs_zero_disk_writes_and_existing_read_enforces_real_os_lock() {
        let root = tempfile::tempdir().unwrap();
        let mcpm_dir = root.path().join(".mcpm");
        let lock_path = server_lock_path(root.path());

        // Missing .mcpm/ and server.lock: acquire_read succeeds with zero writes.
        let read_without_dir = ServerLock::acquire_read(root.path()).unwrap();
        assert!(!mcpm_dir.exists());
        assert!(!lock_path.exists());
        drop(read_without_dir);

        // acquire_existing_read rejects missing server.lock without creating files.
        assert!(ServerLock::acquire_existing_read(root.path()).is_err());
        assert!(!mcpm_dir.exists());

        // Pending journal is still rejected even when server.lock does not exist.
        fs::create_dir(&mcpm_dir).unwrap();
        fs::write(journal_path(root.path()), b"{}").unwrap();
        assert!(ServerLock::acquire_read(root.path()).is_err());
        assert!(!lock_path.exists());
        fs::remove_file(journal_path(root.path())).unwrap();

        // Missing server.lock with existing .mcpm/ still creates no lock file.
        let read_without_file = ServerLock::acquire_read(root.path()).unwrap();
        assert!(!lock_path.exists());
        drop(read_without_file);

        // Create server.lock via write lock, then verify duplicate-fd release on Drop.
        let write = ServerLock::acquire_write(root.path()).unwrap();
        assert!(lock_path.is_file());
        let inherited = write._file.as_ref().unwrap().try_clone().unwrap();
        assert!(ServerLock::acquire_read(root.path()).is_err());
        assert!(ServerLock::acquire_existing_read(root.path()).is_err());
        drop(write);

        // Existing shared lock blocks concurrent exclusive writes while allowing shared reads.
        let existing_shared = ServerLock::acquire_existing_read(root.path()).unwrap();
        let second_shared = ServerLock::acquire_read(root.path()).unwrap();
        assert!(ServerLock::acquire_write(root.path()).is_err());
        drop(second_shared);
        drop(existing_shared);

        let reacquired = ServerLock::acquire_write(root.path()).unwrap();
        drop(inherited);
        drop(reacquired);
    }

    #[test]
    fn commit_keep_conf_leaves_conf_unread_and_rejects_pending_journal_when_recovery_disabled() {
        use crate::local::{
            java::command::JavaEntry,
            mc::server::{
                SERVER_METADATA_VERSION, ServerLoader,
                config::{Arguments, Process},
            },
        };

        let root = tempfile::tempdir().unwrap();
        let conf_path = root.path().join("conf.toml");
        // Non-UTF-8 and invalid TOML proves ConfAction::Keep never reads or parses conf.toml.
        let conf_bytes = b"\xff# raw user bytes\r\n[not valid toml";
        fs::write(&conf_path, conf_bytes).unwrap();

        let identity = MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: "1.21.1".into(),
            loader: ServerLoader::Vanilla,
        };
        let launch = LaunchConfig {
            version: 1,
            platform: None,
            process: Process::default(),
            jvm: Arguments::default(),
            entry: JavaEntry::Jar {
                path: "server.jar".into(),
            },
            application: Arguments::default(),
        };

        let lock = ServerLock::acquire_write(root.path()).unwrap();
        let pending = Journal {
            version: 1,
            files: vec![Change {
                target: Target::Metadata,
                before: None,
                after: Some(b"pending".to_vec()),
            }],
        };
        replace(
            &journal_path(root.path()),
            &serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();

        let plan = Publication {
            identity: &identity,
            launch: LaunchAction::Write(&launch),
            conf: ConfAction::Keep,
            replace_existing: true,
            recover_pending: false,
        };
        assert!(commit(root.path(), &lock, plan).is_err());
        assert!(journal_path(root.path()).exists());
        assert_eq!(fs::read(&conf_path).unwrap(), conf_bytes);

        assert!(recover_if_pending(root.path(), &lock).unwrap());
        let published = commit(
            root.path(),
            &lock,
            Publication {
                identity: &identity,
                launch: LaunchAction::Write(&launch),
                conf: ConfAction::Keep,
                replace_existing: true,
                recover_pending: false,
            },
        )
        .unwrap();
        assert!(published.entry_configured);
        assert!(published.imported.is_empty());
        assert_eq!(published.preserved, vec![conf_path.clone()]);
        assert_eq!(fs::read(&conf_path).unwrap(), conf_bytes);
        assert!(Target::Metadata.path(root.path()).is_file());
        assert!(Target::Launch.path(root.path()).is_file());
    }
}
