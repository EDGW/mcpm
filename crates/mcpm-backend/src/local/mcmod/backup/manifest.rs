//! The sole manifest codec and order-independent semantic checksum protocol.

use std::{collections::BTreeSet, fs::OpenOptions, io::Write, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::local::files::{Content, hash::Blake3Hash};

pub const MANIFEST_NAME: &str = "manifest.mcbackup";
const VERSION: u32 = 1;
const DOMAIN: &[u8] = b"mcpm.mcbackup\0";

/// A validated manifest. JSON representation is owned exclusively by this module.
#[derive(Debug, Clone)]
pub struct Manifest {
    document: Document,
}

impl Manifest {
    pub(crate) fn new(files: Vec<FileRecord>) -> Result<Self> {
        let mut document = Document {
            version: VERSION,
            files,
            checksum: Blake3Hash::new([0; 32]),
        };
        normalize(&mut document)?;
        document.checksum = checksum(&document);
        Ok(Self { document })
    }

    /// Parses, validates and sorts records before comparing the semantic checksum.
    pub fn read(path: &Path) -> Result<Self> {
        Ok(Self::read_with_size(path)?.0)
    }

    /// Returns the original encoded size, including its whitespace and escaping.
    pub(super) fn read_with_size(path: &Path) -> Result<(Self, u64)> {
        let mut content = Content::open(path)?
            .with_context(|| format!("backup manifest does not exist: {path:?}"))?;
        let size = content.size();
        let mut document: Document = serde_json::from_slice(&content.read()?)
            .with_context(|| format!("failed to decode backup manifest {path:?}"))?;
        content.validate()?;
        normalize(&mut document)?;
        let actual = checksum(&document);
        if actual != document.checksum {
            return Err(ChecksumMismatch {
                expected: document.checksum,
                actual,
            }
            .into());
        }
        Ok((Self { document }, size))
    }

    pub fn files(&self) -> &[FileRecord] {
        &self.document.files
    }

    pub fn checksum(&self) -> Blake3Hash {
        self.document.checksum
    }

    pub(crate) fn write(&self, path: &Path) -> Result<u64> {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("failed to create backup manifest {path:?}"))?;
        serde_json::to_writer_pretty(&mut output, &self.document)
            .context("failed to encode backup manifest")?;
        output.write_all(b"\n")?;
        output
            .sync_all()
            .context("failed to flush backup manifest")?;
        Ok(output.metadata()?.len())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("backup manifest checksum is {actual}; expected {expected}")]
pub struct ChecksumMismatch {
    pub expected: Blake3Hash,
    pub actual: Blake3Hash,
}

/// A file's relative name and immutable cache identity; directories have no records.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    path: String,
    blake3: Blake3Hash,
    size: u64,
}

impl FileRecord {
    pub(crate) fn new(path: String, blake3: Blake3Hash, size: u64) -> Self {
        Self { path, blake3, size }
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn blake3(&self) -> Blake3Hash {
        self.blake3
    }
    pub fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    files: Vec<FileRecord>,
    checksum: Blake3Hash,
}

/// Portable separators and canonical relative components, without filesystem access.
pub(super) fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && !path.contains(['\\', '\0']),
        "invalid backup file path {path:?}"
    );
    for component in path.split('/') {
        ensure!(
            !matches!(component, "" | "." | ".."),
            "invalid backup file path {path:?}"
        );
    }
    // Drive-relative names must never be interpreted as a Windows path prefix.
    ensure!(
        !path.split('/').next().unwrap().contains(':'),
        "invalid backup file path {path:?}"
    );
    Ok(())
}

fn normalize(document: &mut Document) -> Result<()> {
    ensure!(
        document.version == VERSION,
        "unsupported backup version {}",
        document.version
    );
    document.files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let mut paths = BTreeSet::new();
    for file in &document.files {
        validate_path(&file.path)?;
        ensure!(
            paths.insert(file.path.as_str()),
            "duplicate backup file path {:?}",
            file.path
        );
    }
    for path in &paths {
        for (offset, _) in path.match_indices('/') {
            ensure!(
                !paths.contains(&path[..offset]),
                "backup file is also a parent directory: {:?}",
                &path[..offset]
            );
        }
    }
    Ok(())
}

fn checksum(document: &Document) -> Blake3Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN);
    hasher.update(&document.version.to_be_bytes());
    hasher.update(&(document.files.len() as u64).to_be_bytes());
    for file in &document.files {
        hasher.update(&(file.path.len() as u64).to_be_bytes());
        hasher.update(file.path.as_bytes());
        hasher.update(file.blake3.as_bytes());
        hasher.update(&file.size.to_be_bytes());
    }
    Blake3Hash::new(*hasher.finalize().as_bytes())
}
