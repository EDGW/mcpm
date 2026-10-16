use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;
use zip::ZipArchive;

use crate::local::{
    mcmod::{ModInfo, ModMeta, NestedModMeta},
    modloader::ModLoaderMeta,
};

use super::{Archive, ArchiveReader};

// Implementation limits, not loader metadata restrictions.
const MAX_DEPTH: usize = 32;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVES: usize = 4096;

#[derive(Default)]
pub(in crate::local::modloader::impls) struct ArchiveNode {
    pub mods: Vec<ModInfo>,
    pub jars: Vec<String>,
}

#[derive(Debug, Error)]
enum NestedArchiveError {
    #[error("invalid nested JAR path {path:?}: expected a relative archive member path")]
    InvalidPath { path: String },
    #[error("nested JAR traversal exceeds the {limit} level depth limit")]
    DepthLimit { limit: usize },
    #[error("nested JAR traversal exceeds the {limit} archive limit")]
    ArchiveLimit { limit: usize },
    #[error("nested JAR traversal exceeds the {limit} byte read limit")]
    ByteLimit { limit: u64 },
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
        if member.is_empty()
            || member.starts_with('/')
            || member.split('/').any(|part| part == "..")
        {
            return Err(NestedArchiveError::InvalidPath { path: member }.into());
        }
        if depth >= MAX_DEPTH {
            return Err(NestedArchiveError::DepthLimit { limit: MAX_DEPTH }.into());
        }
        if budget.archives >= MAX_ARCHIVES {
            return Err(NestedArchiveError::ArchiveLimit {
                limit: MAX_ARCHIVES,
            }
            .into());
        }
        budget.archives += 1;
        let child_display_path = PathBuf::from(format!("{}!{member}", path.display()));
        let mut nested = read_nested(archive, &member, budget)
            .with_context(|| format!("failed to open nested JAR {child_display_path:?}"))?;
        let node = identify(&mut nested, &child_display_path)?;
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
    let mut file = archive.by_name(member)?;
    let remaining = MAX_BYTES - budget.bytes;
    if file.size() > remaining {
        return Err(NestedArchiveError::ByteLimit { limit: MAX_BYTES }.into());
    }
    let mut bytes = Vec::new();
    (&mut file).take(remaining + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > remaining {
        return Err(NestedArchiveError::ByteLimit { limit: MAX_BYTES }.into());
    }
    budget.bytes += bytes.len() as u64;
    Ok(ZipArchive::new(
        Box::new(Cursor::new(bytes)) as Box<dyn ArchiveReader>
    )?)
}
