//! Sole owner of canonical content names, metadata suffixes and shard validation.

use super::hash::Blake3Hash;
use anyhow::{Context, Result, ensure};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(super) const MANIFEST: &str = ".mcpm/metadata.json";
const METADATA_SUFFIX: &str = ".json";

pub(super) fn data_path(root: &Path, key: &Blake3Hash) -> PathBuf {
    let name = key.to_string();
    root.join(&name[..2]).join(name)
}

pub(super) fn sidecar_path(root: &Path, key: &Blake3Hash, name: &str) -> Result<PathBuf> {
    ensure!(valid_metaname(name), "invalid cache metadata name {name:?}");
    let mut path = data_path(root, key).into_os_string();
    path.push(".");
    path.push(name);
    path.push(METADATA_SUFFIX);
    Ok(path.into())
}

/// Extracts the metadata name from the portion following `<blake3>.`.
pub(super) fn metaname_from_suffix(suffix: &str) -> Option<&str> {
    let name = suffix.strip_suffix(METADATA_SUFFIX)?;
    valid_metaname(name).then_some(name)
}

fn valid_metaname(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}

/// Parses both content names and owned sidecars; rejects misplaced and nested paths.
pub(super) fn key_from_path(root: &Path, path: &Path) -> Option<Blake3Hash> {
    let mut parts = path.strip_prefix(root).ok()?.components();
    let shard = parts.next()?.as_os_str().to_str()?;
    let filename = parts.next()?.as_os_str().to_str()?;
    if parts.next().is_some() {
        return None;
    }
    let (name, suffix) = filename
        .split_once('.')
        .map_or((filename, None), |(key, suffix)| (key, Some(suffix)));
    let key: Blake3Hash = name.parse().ok()?;
    if shard != &name[..2] || suffix.is_some_and(|suffix| metaname_from_suffix(suffix).is_none()) {
        return None;
    }
    Some(key)
}

/// Missing shards return false; existing shards must be real directories.
pub(super) fn shard_exists(root: &Path, key: &Blake3Hash) -> Result<bool> {
    let path = data_path(root, key);
    let parent = path.parent().expect("cache content has a shard");
    match fs::symlink_metadata(parent) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("failed to inspect cache shard {parent:?}"))
        }
        Ok(metadata) => {
            ensure!(
                metadata.is_dir(),
                "expected a cache shard directory without symbolic links: {parent:?}"
            );
            Ok(true)
        }
    }
}

pub(super) fn create_shard(root: &Path, key: &Blake3Hash) -> Result<()> {
    let path = data_path(root, key);
    let parent = path.parent().expect("cache content has a shard");
    match fs::create_dir(parent) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to create cache shard {parent:?}"));
        }
    }
    shard_exists(root, key)?;
    Ok(())
}

pub(super) fn valid_shard_name(name: &str) -> bool {
    name.len() == 2
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
