//! Locked multi-file publication with durable preimages and rollback on failure or explicit retry.

use super::*;
use crate::local::mc::server::{MinecraftServer, MinecraftServerMetadata, metadata};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Separate overwrite intents; replacing conf additionally requires replacing registration.
#[derive(Debug, Clone, Copy, Default)]
pub struct OverwritePolicy {
    pub replace_registration: bool,
    pub replace_conf: bool,
    /// Permit keeping an existing conf when overrides are only defaults for a new file.
    pub preserve_conf_with_overrides: bool,
}

impl OverwritePolicy {
    fn validate(self) -> Result<()> {
        ensure!(
            !self.replace_conf || self.replace_registration,
            "replacing conf.toml requires explicit registration replacement as well"
        );
        Ok(())
    }
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

/// Publishes local registration and configuration under the backend's own coordination lock.
/// A journal is removed only after all files have been installed. An interrupted publication
/// is rolled back on the next explicit recovery or publication; launch refuses a pending journal.
pub fn publish(
    root: &Path,
    identity: &MinecraftServerMetadata,
    draft: Option<&ConfigurationDraft>,
    conf_overrides: &ConfigurationOverrides,
    policy: OverwritePolicy,
) -> Result<Published> {
    let prepared = prepare_publication(root, identity, draft, conf_overrides, policy)?;
    let lock = ConfigurationLock::acquire(root, true)?;
    commit_publication(root, &lock, prepared, draft, conf_overrides, policy)
}

/// Publishes local registration and configuration while the caller already holds an exclusive
/// [`ConfigurationLock`].
///
/// Any pending journal is rolled back before publishing. Callers that must abort after
/// recovering an interrupted publication should call [`recover_if_pending`] first.
pub fn publish_locked(
    root: &Path,
    lock: &ConfigurationLock,
    identity: &MinecraftServerMetadata,
    draft: Option<&ConfigurationDraft>,
    conf_overrides: &ConfigurationOverrides,
    policy: OverwritePolicy,
) -> Result<Published> {
    let prepared = prepare_publication(root, identity, draft, conf_overrides, policy)?;
    commit_publication(root, lock, prepared, draft, conf_overrides, policy)
}

/// Recovers a pending configuration publication journal while holding an exclusive
/// [`ConfigurationLock`].
///
/// Returns `Ok(true)` if a pending journal existed and its preimages were restored, or
/// `Ok(false)` if no pending journal was present.
pub fn recover_if_pending(root: &Path, lock: &ConfigurationLock) -> Result<bool> {
    lock.ensure_write_for(root, "configuration recovery")?;
    recover(root).context("failed to recover interrupted configuration publication")
}

struct PreparedPublication {
    identity: MinecraftServerMetadata,
    metadata_bytes: Vec<u8>,
    launch_text: Option<String>,
}

fn prepare_publication(
    root: &Path,
    identity: &MinecraftServerMetadata,
    draft: Option<&ConfigurationDraft>,
    conf_overrides: &ConfigurationOverrides,
    policy: OverwritePolicy,
) -> Result<PreparedPublication> {
    policy.validate()?;
    ensure!(
        conf_overrides.is_empty() || draft.is_none_or(|draft| draft.imported.is_empty()),
        "explicit conf overrides cannot be combined with imported script settings"
    );
    ensure!(
        root.is_dir(),
        "server root is not an existing directory: {root:?}"
    );
    let (identity, metadata_bytes) = metadata::encode(root, identity)?;
    let launch_text = draft.map(|draft| draft.launch.encode()).transpose()?;
    Ok(PreparedPublication {
        identity,
        metadata_bytes,
        launch_text,
    })
}

fn commit_publication(
    root: &Path,
    lock: &ConfigurationLock,
    prepared: PreparedPublication,
    draft: Option<&ConfigurationDraft>,
    conf_overrides: &ConfigurationOverrides,
    policy: OverwritePolicy,
) -> Result<Published> {
    lock.ensure_write_for(root, "configuration publication")?;
    recover(root).context(
        "failed to recover interrupted configuration publication; no files were modified",
    )?;
    let server = MinecraftServer::registration_snapshot(root.to_owned(), prepared.identity)?;
    let old_metadata = existing(&root.join(".mcpm/server.json"))?;
    let old_launch = existing(&root.join("launch.toml"))?;
    let old_conf = existing(&root.join("conf.toml"))?;
    ensure!(
        policy.replace_registration || (old_metadata.is_none() && old_launch.is_none()),
        "server registration or launch.toml already exists; use --force to import again"
    );
    let preserve_conf = old_conf.is_some() && !policy.replace_conf;
    ensure!(
        !preserve_conf || policy.preserve_conf_with_overrides || conf_overrides.is_empty(),
        "conf.toml already exists; use --force --foverride-conf to apply --conf"
    );
    let (conf_text, effective_conf) = if preserve_conf {
        let conf = decode_conf(
            std::str::from_utf8(old_conf.as_deref().unwrap())
                .context("preserved conf.toml is not UTF-8")?,
        )
        .context("preserved conf.toml is invalid; fix it or explicitly override it")?;
        (None, conf)
    } else if !conf_overrides.is_empty() {
        let (text, conf) = import::manual_template(conf_overrides)?;
        (Some(text), conf)
    } else {
        let text = match draft {
            Some(draft) => draft.template()?,
            None => import::default_template(),
        };
        let conf = decode_conf(&text)?;
        (Some(text), conf)
    };
    if let Some(draft) = draft {
        validate_pair(&draft.launch, &effective_conf)
            .context("new launch structure conflicts with effective conf.toml")?;
    }
    let mut journal = Journal {
        version: 1,
        files: vec![Change {
            target: Target::Metadata,
            before: old_metadata,
            after: Some(prepared.metadata_bytes),
        }],
    };
    if prepared.launch_text.is_some() || old_launch.is_some() {
        journal.files.push(Change {
            target: Target::Launch,
            before: old_launch,
            after: prepared.launch_text.map(String::into_bytes),
        });
    }
    if let Some(text) = conf_text {
        journal.files.push(Change {
            target: Target::Conf,
            before: old_conf,
            after: Some(text.into_bytes()),
        });
    }
    let journal_path = journal_path(root);
    let bytes =
        serde_json::to_vec(&journal).context("failed to encode configuration recovery journal")?;
    replace(&journal_path, &bytes).context("failed to prepare configuration recovery journal")?;
    let commit = (|| -> Result<()> {
        for change in &journal.files {
            install(&change.target.path(root), change.after.as_deref())?;
        }
        fs::remove_file(&journal_path).context("failed to finish configuration publication")?;
        Ok(())
    })();
    if let Err(source) = commit {
        if let Err(recovery) = recover(root) {
            return Err(PublicationRecoveryError {
                errors: mcpm_exec::error::CombinedError::new(source, recovery),
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
        imported: draft
            .map(|draft| draft.imported.clone())
            .unwrap_or_default(),
        entry_configured: draft.is_some(),
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
    if preserve_conf {
        result.preserved.push(root.join("conf.toml"));
    }
    Ok(result)
}

/// Preserves both independent error chains if automatic rollback also fails.
#[derive(Debug, Error)]
#[error(
    "configuration publication and recovery failed; retry the interrupted operation to recover from {journal:?}"
)]
pub struct PublicationRecoveryError {
    #[source]
    pub errors: mcpm_exec::error::CombinedError,
    pub journal: PathBuf,
}

/// Coordination lock on `.mcpm/launch.lock` serializing configuration publication and reads.
///
/// Global lock order: callers MUST acquire `.mcpm/run/launch.lock` before acquiring
/// `ConfigurationLock` (`.mcpm/launch.lock`).
pub struct ConfigurationLock {
    _file: File,
    root: PathBuf,
    write: bool,
}

impl ConfigurationLock {
    /// Acquires an exclusive write lock on `.mcpm/launch.lock`.
    ///
    /// Global lock order: callers MUST acquire `.mcpm/run/launch.lock` before acquiring
    /// `ConfigurationLock` (`.mcpm/launch.lock`).
    pub fn acquire_write(root: &Path) -> Result<Self> {
        Self::acquire(root, true)
    }

    pub(crate) fn acquire(root: &Path, write: bool) -> Result<Self> {
        ensure!(
            root.is_dir(),
            "server root is not an existing directory: {root:?}"
        );
        let canonical_root =
            fs::canonicalize(root).with_context(|| format!("failed to resolve server root {root:?}"))?;
        let mcpm_dir = canonical_root.join(".mcpm");
        if write {
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
        }
        let lock_path = mcpm_dir.join("launch.lock");
        match fs::symlink_metadata(&lock_path) {
            Ok(meta) => ensure!(
                meta.is_file(),
                "server configuration lock path is not a regular file: {lock_path:?}"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to inspect server configuration lock"),
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .context("failed to open server configuration lock")?;
        if write {
            file.try_lock()
        } else {
            file.try_lock_shared()
        }
        .context("server configuration is busy or cannot be locked")?;
        if !write {
            ensure!(
                !journal_path(&canonical_root).try_exists()?,
                "an interrupted configuration publication requires recovery; retry the interrupted operation (init --force or add-loader) before launching"
            );
        }
        Ok(Self {
            _file: file,
            root: canonical_root,
            write,
        })
    }

    fn ensure_write_for(&self, root: &Path, action: &str) -> Result<()> {
        ensure!(
            self.write,
            "{action} requires an exclusive configuration lock"
        );
        let canonical =
            fs::canonicalize(root).with_context(|| format!("failed to resolve server root {root:?}"))?;
        ensure!(
            self.root == canonical,
            "{action} lock belongs to {:?}, not {:?}",
            self.root,
            canonical
        );
        Ok(())
    }

    pub(crate) fn require_clean(root: &Path) -> Result<()> {
        ensure!(
            !journal_path(root).try_exists()?,
            "interrupted configuration publication requires explicit recovery through init --force or add-loader"
        );
        Ok(())
    }
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

fn existing(path: &Path) -> Result<Option<Vec<u8>>> {
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
        assert!(ConfigurationLock::acquire(root.path(), false).is_err());
        let lock = ConfigurationLock::acquire_write(root.path()).unwrap();
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
        assert!(ConfigurationLock::acquire_write(&missing).is_err());
        assert!(!missing.exists());

        let shared = ConfigurationLock::acquire(root.path(), false).unwrap();
        assert!(recover_if_pending(root.path(), &shared).is_err());
        drop(shared);

        let other = tempfile::tempdir().unwrap();
        let other_lock = ConfigurationLock::acquire_write(other.path()).unwrap();
        assert!(recover_if_pending(root.path(), &other_lock).is_err());
    }
}
