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
/// is rolled back on the next explicit import; launch refuses a pending journal.
pub fn publish(
    root: &Path,
    identity: &MinecraftServerMetadata,
    draft: Option<&ConfigurationDraft>,
    policy: OverwritePolicy,
) -> Result<Published> {
    policy.validate()?;
    ensure!(
        root.is_dir(),
        "server root is not an existing directory: {root:?}"
    );
    let (identity, metadata_bytes) = metadata::encode(root, identity)?;
    let launch_text = draft.map(|draft| draft.launch.encode()).transpose()?;
    let conf_text = match draft {
        Some(draft) => draft.template()?,
        None => import::default_template()?,
    };
    let _lock = ConfigurationLock::acquire(root, true)?;
    recover(root).context(
        "failed to recover interrupted configuration publication; files were not imported",
    )?;
    let server = MinecraftServer::registration_snapshot(root.to_owned(), identity)?;
    let old_metadata = existing(&root.join(".mcpm/server.json"))?;
    let old_launch = existing(&root.join("launch.toml"))?;
    let old_conf = existing(&root.join("conf.toml"))?;
    ensure!(
        policy.replace_registration || (old_metadata.is_none() && old_launch.is_none()),
        "server registration or launch.toml already exists; use --force to import again"
    );
    let preserve_conf = old_conf.is_some() && !policy.replace_conf;
    let effective_conf = if preserve_conf {
        decode_conf(
            std::str::from_utf8(old_conf.as_deref().unwrap())
                .context("preserved conf.toml is not UTF-8")?,
        )
        .context("preserved conf.toml is invalid; fix it or explicitly override it")?
    } else {
        decode_conf(&conf_text)?
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
            after: Some(metadata_bytes),
        }],
    };
    if launch_text.is_some() || old_launch.is_some() {
        journal.files.push(Change {
            target: Target::Launch,
            before: old_launch,
            after: launch_text.map(String::into_bytes),
        });
    }
    if !preserve_conf {
        journal.files.push(Change {
            target: Target::Conf,
            before: old_conf,
            after: Some(conf_text.into_bytes()),
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
#[error("configuration publication and recovery failed; retry import to recover from {journal:?}")]
pub struct PublicationRecoveryError {
    #[source]
    pub errors: crate::error::CombinedError,
    pub journal: PathBuf,
}

pub(crate) struct ConfigurationLock {
    _file: File,
}

impl ConfigurationLock {
    pub(crate) fn acquire(root: &Path, write: bool) -> Result<Self> {
        if write {
            fs::create_dir_all(root.join(".mcpm"))
                .context("failed to create server metadata directory")?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".mcpm/launch.lock"))
            .context("failed to open server configuration lock")?;
        if write {
            file.try_lock()
        } else {
            file.try_lock_shared()
        }
        .context("server configuration is busy or cannot be locked")?;
        if !write {
            ensure!(
                !journal_path(root).try_exists()?,
                "an interrupted configuration import requires recovery; explicitly run init --force again before launching"
            );
        }
        Ok(Self { _file: file })
    }

    pub(crate) fn require_clean(root: &Path) -> Result<()> {
        ensure!(
            !journal_path(root).try_exists()?,
            "interrupted configuration import requires explicit recovery through server initialization"
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

fn recover(root: &Path) -> Result<()> {
    let path = journal_path(root);
    let Some(bytes) = existing(&path)? else {
        return Ok(());
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
    sync_directory(&root.join(".mcpm"))
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
        let _lock = ConfigurationLock::acquire(root.path(), true).unwrap();
        recover(root.path()).unwrap();
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
        recover(root.path()).unwrap();
    }
}
