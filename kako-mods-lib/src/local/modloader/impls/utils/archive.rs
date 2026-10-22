//! Mod archive adapters and declared nested JAR traversal with shared resource budgets.

use std::collections::HashSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;

use crate::local::{
    archive::zip::ArchiveError,
    mcmod::{ModEntry, ModInfo, ModMeta, NestedModMeta},
    modloader::ModLoaderMeta,
};

pub(in crate::local::modloader::impls) use crate::local::archive::zip::Archive;
pub(in crate::local::modloader::impls) use crate::local::java::jar::read_text as read;

// Implementation limits, not loader metadata restrictions.
const MAX_DEPTH: usize = 32;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVES: usize = 4096;

#[derive(Default, Debug)]
pub(in crate::local::modloader::impls) struct ArchiveNode {
    pub mods: Vec<ModInfo>,
    pub jars: Vec<String>,
}

pub(in crate::local::modloader::impls) fn collect(
    archive: &mut Archive,
    root: ArchiveNode,
    path: &Path,
    loader: &ModLoaderMeta,
    mut identify: impl FnMut(&mut Archive, &Path) -> Result<ArchiveNode>,
) -> Result<ModMeta> {
    let mut budget = Budget {
        bytes: 0,
        archives: 0,
    };
    visit(archive, root, path, 0, loader, &mut budget, &mut identify)
}

pub(in crate::local::modloader::impls) fn open(entry: &ModEntry) -> Result<Archive> {
    Archive::open(entry.path())
        .with_context(|| format!("failed to open mod archive {:?}", entry.path()))
}

#[derive(Debug)]
struct Budget {
    bytes: u64,
    archives: usize,
}

fn visit(
    archive: &mut Archive,
    node: ArchiveNode,
    path: &Path,
    depth: usize,
    loader: &ModLoaderMeta,
    budget: &mut Budget,
    identify: &mut impl FnMut(&mut Archive, &Path) -> Result<ArchiveNode>,
) -> Result<ModMeta> {
    let mut metadata = ModMeta {
        loader: loader.clone(),
        mods: node.mods,
        nested: Vec::new(),
    };
    let mut seen = HashSet::new();
    for member in node.jars {
        if !seen.insert(member.clone()) {
            continue;
        }
        let child_display_path = PathBuf::from(format!("{}!{member}", path.display()));
        if member.is_empty()
            || member.starts_with('/')
            || member.split('/').any(|part| part == "..")
        {
            return Err(NestedArchiveError::InvalidPath {
                path: child_display_path,
            }
            .into());
        }
        if depth >= MAX_DEPTH {
            return Err(NestedArchiveError::DepthLimit {
                limit: MAX_DEPTH,
                path: child_display_path,
            }
            .into());
        }
        if budget.archives >= MAX_ARCHIVES {
            return Err(NestedArchiveError::ArchiveLimit {
                limit: MAX_ARCHIVES,
                path: child_display_path,
            }
            .into());
        }
        budget.archives += 1;
        let mut nested = read_nested(archive, &member, budget)
            .with_context(|| format!("failed to open nested JAR {child_display_path:?}"))?;
        let node = identify(&mut nested, &child_display_path)
            .with_context(|| format!("failed to identify nested JAR {child_display_path:?}"))?;
        let meta = visit(
            &mut nested,
            node,
            &child_display_path,
            depth + 1,
            loader,
            budget,
            identify,
        )?;
        metadata.nested.push(NestedModMeta { path: member, meta });
    }
    Ok(metadata)
}

fn read_nested(archive: &mut Archive, member: &str, budget: &mut Budget) -> Result<Archive> {
    let remaining = MAX_BYTES - budget.bytes;
    let bytes = archive
        .read(member, Some(remaining))
        .map_err(|source| match source {
            ArchiveError::SizeLimit { .. } => {
                anyhow::Error::new(NestedArchiveError::ByteLimit { limit: MAX_BYTES })
            }
            source => source.into(),
        })?
        .with_context(|| format!("declared nested JAR {member:?} is missing"))?;
    budget.bytes += bytes.len() as u64;
    Ok(Archive::from_reader(Cursor::new(bytes))?)
}

#[derive(Debug, Error)]
enum NestedArchiveError {
    #[error("invalid nested JAR path {path:?}: expected a relative archive member path")]
    InvalidPath { path: PathBuf },
    #[error("nested JAR traversal exceeds the {limit} level depth limit at {path:?}")]
    DepthLimit { limit: usize, path: PathBuf },
    #[error("nested JAR traversal exceeds the {limit} archive limit at {path:?}")]
    ArchiveLimit { limit: usize, path: PathBuf },
    #[error("nested JAR traversal exceeds the {limit} byte read limit")]
    ByteLimit { limit: u64 },
}
