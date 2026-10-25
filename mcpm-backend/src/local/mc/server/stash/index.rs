//! Sole JSON codec and atomic publication for stash records and HEAD; paths are UUID-owned.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A validated stash identity and its independently owned backup reference.
#[derive(Debug, Clone)]
pub struct Stash {
    document: Document,
}

impl Stash {
    pub(super) fn new(name: String) -> Result<Self> {
        validate_name(&name)?;
        Ok(Self {
            document: Document {
                version: 1,
                id: Uuid::new_v4(),
                name,
                created_at: std::time::SystemTime::now().into(),
                backup: Uuid::new_v4(),
            },
        })
    }

    pub fn id(&self) -> Uuid {
        self.document.id
    }
    pub fn name(&self) -> &str {
        &self.document.name
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.document.created_at
    }
    pub fn backup_id(&self) -> Uuid {
        self.document.backup
    }

    pub(super) fn rename(&mut self, name: String) -> Result<()> {
        validate_name(&name)?;
        self.document.name = name;
        Ok(())
    }
}

/// Storage paths and codecs only; the caller owns the lifetime of the operation lock.
pub(super) struct Index {
    root: PathBuf,
    available: bool,
}

impl Index {
    pub fn new(root: PathBuf, available: bool) -> Self {
        Self { root, available }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read(&self, id: Uuid) -> Result<Stash> {
        ensure!(self.available, "stash {id} does not exist");
        directory(&self.root.join("index"), false)?;
        let path = self.record_path(id);
        let document: Document = serde_json::from_slice(&read_regular(&path)?)
            .with_context(|| format!("invalid stash index {path:?}"))?;
        ensure!(
            document.version == 1,
            "unsupported stash version {}",
            document.version
        );
        ensure!(
            document.id == id,
            "stash UUID differs from its index filename: {path:?}"
        );
        ensure!(
            !document.id.is_nil() && !document.backup.is_nil(),
            "nil stash or backup UUID: {path:?}"
        );
        validate_name(&document.name)?;
        Ok(Stash { document })
    }

    pub fn list(&self) -> Result<Vec<Result<Stash>>> {
        if !self.available || !directory(&self.root.join("index"), false)? {
            return Ok(Vec::new());
        }
        let mut paths = fs::read_dir(self.root.join("index"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        paths.sort();
        Ok(paths
            .into_iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .map(|path| {
                let stem = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .context("stash index filename is not UTF-8")?;
                let id: Uuid = stem
                    .parse()
                    .with_context(|| format!("invalid stash index filename {path:?}"))?;
                ensure!(
                    stem == id.to_string(),
                    "noncanonical stash index filename: {path:?}"
                );
                self.read(id)
            })
            .collect())
    }

    pub fn publish(&self, stash: &Stash, replace: bool) -> Result<()> {
        directory(&self.root.join("index"), true)?;
        publish(&self.record_path(stash.id()), &stash.document, replace)
    }

    pub fn remove(&self, id: Uuid) -> Result<()> {
        fs::remove_file(self.record_path(id))
            .with_context(|| format!("failed to remove stash index {id}"))
    }

    pub fn head(&self) -> Result<Option<Uuid>> {
        if !self.available {
            return Ok(None);
        }
        let path = self.root.join("HEAD");
        if !regular_exists(&path)? {
            return Ok(None);
        }
        let head: Head =
            serde_json::from_slice(&read_regular(&path)?).context("invalid stash HEAD")?;
        ensure!(
            head.version == 1,
            "unsupported stash HEAD version {}",
            head.version
        );
        ensure!(!head.stash.is_nil(), "nil stash UUID in HEAD");
        Ok(Some(head.stash))
    }

    pub fn set_head(&self, stash: &Stash) -> Result<()> {
        publish(
            &self.root.join("HEAD"),
            &Head {
                version: 1,
                stash: stash.id(),
            },
            true,
        )
    }

    fn record_path(&self, id: Uuid) -> PathBuf {
        self.root.join("index").join(format!("{id}.json"))
    }
}

pub(super) fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty() && !name.chars().any(char::is_control),
        "stash name must contain non-whitespace text and no control characters"
    );
    Ok(())
}

/// Creates only a missing ordinary directory; symlink roots are never adopted.
pub(super) fn directory(path: &Path, create: bool) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir(),
                "expected a directory without symbolic links: {path:?}"
            );
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !create {
                return Ok(false);
            }
            match fs::create_dir(path) {
                Ok(()) => Ok(true),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    directory(path, false)
                }
                Err(error) => {
                    Err(error).with_context(|| format!("failed to create directory {path:?}"))
                }
            }
        }
        Err(error) => Err(error).with_context(|| format!("failed to inspect directory {path:?}")),
    }
}

pub(super) fn regular_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file(),
                "expected a regular file without symbolic links: {path:?}"
            );
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect file {path:?}")),
    }
}

fn read_regular(path: &Path) -> Result<Vec<u8>> {
    ensure!(regular_exists(path)?, "file does not exist: {path:?}");
    fs::read(path).with_context(|| format!("failed to read file {path:?}"))
}

fn publish(path: &Path, value: &impl Serialize, replace: bool) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).context("failed to encode stash data")?;
    bytes.push(b'\n');
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    if replace {
        // Validate existing targets before replacement as well as on read.
        regular_exists(path)?;
        temporary.persist(path).map_err(|error| error.error)?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|error| error.error)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
    backup: Uuid,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Head {
    version: u32,
    stash: Uuid,
}
