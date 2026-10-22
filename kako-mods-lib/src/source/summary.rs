//! Portable file summaries separating byte access from provider HTTP identification.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{fs::File, io::Read, path::Path};

/// SHA-1 and size are always computed; optional fingerprints are requested by sources.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryRequirements {
    #[serde(default)]
    pub murmur2: bool,
}

/// File contents are represented by these values, never by filenames or JAR bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSummary {
    pub size: u64,
    pub sha1: String,
    /// MurmurHash2 of bytes excluding TAB, LF, CR and SPACE, seeded with 1.
    pub murmur2: Option<u32>,
}

impl FileSummary {
    /// Read bytes once to compute only the requested fingerprints; never performs network I/O.
    pub fn read(path: &Path, requirements: SummaryRequirements) -> Result<Self> {
        let mut file =
            File::open(path).with_context(|| format!("failed to open local file {path:?}"))?;
        ensure!(
            file.metadata()?.is_file(),
            "local path must be a regular file"
        );
        let mut sha1 = Sha1::new();
        let mut filtered = Vec::new();
        let mut buffer = [0; 65536];
        let mut size = 0;
        loop {
            let count = file
                .read(&mut buffer)
                .context("failed to read local file")?;
            if count == 0 {
                break;
            }
            size += count as u64;
            sha1.update(&buffer[..count]);
            if requirements.murmur2 {
                filtered.extend(
                    buffer[..count]
                        .iter()
                        .copied()
                        .filter(|b| !matches!(b, 9 | 10 | 13 | 32)),
                );
            }
        }
        Ok(Self {
            size,
            sha1: format!("{:x}", sha1.finalize()),
            murmur2: requirements
                .murmur2
                .then(|| murmur2(&filtered))
                .transpose()?,
        })
    }

    /// Check digest syntax before including it in a provider request.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.sha1.len() == 40 && self.sha1.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid SHA-1 file summary"
        );
        Ok(())
    }
}

fn murmur2(bytes: &[u8]) -> Result<u32> {
    const M: u32 = 0x5bd1e995;
    let length = u32::try_from(bytes.len())
        .context("local file exceeds the fingerprint algorithm's 32-bit length")?;
    let mut hash = 1 ^ length;
    let mut chunks = bytes.chunks_exact(4);
    for chunk in &mut chunks {
        let mut key = u32::from_le_bytes(chunk.try_into().expect("four-byte chunk"));
        key = key.wrapping_mul(M);
        key ^= key >> 24;
        key = key.wrapping_mul(M);
        hash = hash.wrapping_mul(M) ^ key;
    }
    let tail = chunks.remainder();
    if !tail.is_empty() {
        for (index, byte) in tail.iter().enumerate() {
            hash ^= u32::from(*byte) << (8 * index);
        }
        hash = hash.wrapping_mul(M);
    }
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(M);
    hash ^= hash >> 15;
    Ok(hash)
}
