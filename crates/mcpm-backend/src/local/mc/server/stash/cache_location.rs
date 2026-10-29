//! Server-owned stash cache locations and conservative recognition for management guards.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use crate::local::files::FileIdentity;

const CACHE_PATH: &str = ".mcpm/stash/blob";

pub(in crate::local::mc::server) fn cache_path(server: &Path) -> PathBuf {
    server.join(CACHE_PATH)
}

/// Finds the server owning a stash cache path or one of its descendants.
///
/// Checks both supplied and resolved ancestry, including missing cache directories.
/// The presence of `server.json` reserves the location even if its contents are
/// damaged; this guard deliberately does not interpret server or cache metadata.
/// It does not discover reverse symlink references from unrelated directories.
pub fn cache_owner(path: &Path) -> Result<Option<PathBuf>> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("failed to resolve stash cache candidate {path:?}"))?;
    let resolved = resolve(&absolute)?;
    for ancestor in absolute.ancestors().chain(resolved.ancestors()) {
        if !cache_suffix(ancestor) {
            continue;
        }
        let server = ancestor
            .ancestors()
            .nth(3)
            .expect("stash cache has a server");
        let metadata = server.join(".mcpm/server.json");
        match fs::symlink_metadata(&metadata) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect server metadata {metadata:?}"));
            }
        }
        if !ancestor.ends_with(CACHE_PATH) && !same_case_variant(ancestor, &cache_path(server))? {
            continue;
        }
        // An ancestor followed by `..` may no longer contain the final target.
        if resolved.starts_with(resolve(ancestor)?) {
            return Ok(Some(resolve(server)?));
        }
    }
    Ok(None)
}

fn cache_suffix(path: &Path) -> bool {
    let mut components = path.components().rev();
    Path::new(CACHE_PATH).components().rev().all(|expected| {
        components.next().is_some_and(|actual| {
            actual
                .as_os_str()
                .as_encoded_bytes()
                .eq_ignore_ascii_case(expected.as_os_str().as_encoded_bytes())
        })
    })
}

fn same_case_variant(candidate: &Path, expected: &Path) -> Result<bool> {
    // Existing names can be distinct on case-sensitive filesystems. Missing
    // names are conservatively reserved without creating a filesystem probe.
    Ok(match (identity(candidate)?, identity(expected)?) {
        (Some(candidate), Some(expected)) => candidate == expected,
        (None, None) => true,
        _ => false,
    })
}

fn identity(path: &Path) -> Result<Option<FileIdentity>> {
    match FileIdentity::read(path) {
        Ok(identity) => Ok(Some(identity)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to identify cache path {path:?}")),
    }
}

/// Resolves existing components and symlinks while allowing a missing tail for init.
fn resolve(path: &Path) -> Result<PathBuf> {
    resolve_with_links(path, 40)
}

fn resolve_with_links(path: &Path, remaining_links: usize) -> Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Prefix(_) | Component::RootDir => resolved.push(component.as_os_str()),
            Component::Normal(name) => {
                resolved.push(name);
                match fs::canonicalize(&resolved) {
                    Ok(path) => resolved = path,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        // A default cache can alias a stash cache that has not
                        // been initialized yet. canonicalize alone loses that link.
                        match fs::read_link(&resolved) {
                            Ok(target) => {
                                ensure!(
                                    remaining_links > 0,
                                    "too many symbolic links in cache path {path:?}"
                                );
                                let target = if target.is_absolute() {
                                    target
                                } else {
                                    resolved.parent().expect("link has a parent").join(target)
                                };
                                resolved = resolve_with_links(&target, remaining_links - 1)?;
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
                                ) => {}
                            Err(error) => {
                                return Err(error).with_context(|| {
                                    format!("failed to inspect cache link {resolved:?}")
                                });
                            }
                        }
                    }
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("failed to resolve cache path {resolved:?}"));
                    }
                }
            }
        }
    }
    Ok(resolved)
}
