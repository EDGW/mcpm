//! The sole codec and initialization policy for the cache directory manifest.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use super::{CacheError, error::IoContext, storage};

pub const CACHE_DIRECTORY_VERSION: u32 = 2;
const MANIFEST: &str = ".mcpm/mcpm.json";

#[derive(Debug, Clone, Copy)]
pub(super) enum OpenPolicy {
    Existing,
    InitializeEmpty,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
}

/// Checks before creating any coordination files in the target directory.
/// A separate bootstrap lock serializes first initialization, including empty roots.
pub(super) fn prepare(root: &Path, policy: OpenPolicy) -> Result<(), CacheError> {
    if root
        .join(MANIFEST)
        .try_exists()
        .at("inspect cache manifest", root)?
    {
        return validate(root);
    }
    if matches!(policy, OpenPolicy::Existing) {
        return Err(broken(
            root,
            "cache manifest is missing; initialize an empty cache explicitly",
        ));
    }
    // One persistent bootstrap lock per parent also coordinates root spellings on
    // case-insensitive filesystems and processes with different temp directories.
    let parent = root
        .parent()
        .filter(|_| root.file_name().is_some())
        .ok_or_else(|| broken(root, "filesystem root cannot be initialized as a cache"))?;
    fs::create_dir_all(parent).at("create cache parent", parent)?;
    let _bootstrap = bootstrap_lock(parent)?;
    if root
        .join(MANIFEST)
        .try_exists()
        .at("inspect cache manifest", root)?
    {
        return validate(root);
    }
    match fs::read_dir(root) {
        Ok(mut entries) => {
            if let Some(entry) = entries.next() {
                entry.at("inspect cache directory", root)?;
                return Err(broken(root, "nonempty directory has no cache manifest"));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(root).at("create cache directory", root)?;
        }
        result => {
            result.at("inspect cache directory", root)?;
        }
    }
    let metadata_dir = root.join(".mcpm");
    fs::create_dir(&metadata_dir).at("create cache metadata directory", &metadata_dir)?;
    write(root)?;
    validate(root)
}

pub(super) fn validate(root: &Path) -> Result<(), CacheError> {
    let path = root.join(MANIFEST);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(broken(root, "cache manifest is missing"));
        }
        result => result.at("read cache manifest", &path)?,
    };
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| broken(root, &format!("invalid cache manifest: {error}")))?;
    if manifest.version != CACHE_DIRECTORY_VERSION {
        return Err(CacheError::UnsupportedDirectoryVersion {
            path: root.to_owned(),
            version: manifest.version,
        });
    }
    Ok(())
}

fn write(root: &Path) -> Result<(), CacheError> {
    let path = root.join(MANIFEST);
    let mut temp = storage::temporary(path.parent().expect("manifest has a parent"))?;
    // This fixed schema has no fallible custom serializer.
    let bytes = serde_json::to_vec(&Manifest {
        version: CACHE_DIRECTORY_VERSION,
    })
    .expect("manifest serializes");
    temp.write_all(&bytes).at("write cache manifest", &path)?;
    temp.write_all(b"\n").at("write cache manifest", &path)?;
    storage::publish(temp, &path, false)
}

fn bootstrap_lock(parent: &Path) -> Result<File, CacheError> {
    let path = parent.join(".mcpm-cache-init.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .at("open cache bootstrap lock", &path)?;
    file.lock().at("lock cache initialization", &path)?;
    Ok(file)
}

fn broken(root: &Path, reason: &str) -> CacheError {
    CacheError::BrokenDirectory {
        path: root.to_owned(),
        reason: reason.to_owned(),
    }
}
