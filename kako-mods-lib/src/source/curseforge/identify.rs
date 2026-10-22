//! CurseForge fingerprint lookup with independent size and SHA-1 verification.

use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha1::{Digest, Sha1};

use super::{CurseForgeSource, api};
use crate::source::{FileHash, FileId, FileMatch, FileRef, MatchedFileRole, ProjectId, ProjectRef};

impl CurseForgeSource {
    pub(super) async fn identify_local_file(&self, path: &Path) -> Result<Option<FileMatch>> {
        let path = path.to_owned();
        let (size, sha1, fingerprint) =
            tokio::task::spawn_blocking(move || fingerprint_file(&path))
                .await
                .context("local file fingerprint task failed")??;
        let response: api::Response<Matches> = self
            .request_with_method(reqwest::Method::POST, "fingerprints/432")?
            .json(&serde_json::json!({"fingerprints": [fingerprint]}))
            .send()
            .await
            .context("CurseForge file identification failed")?
            .error_for_status()?
            .json()
            .await
            .context("invalid CurseForge fingerprint response")?;
        let mut matched = None;
        for candidate in response.data.exact_matches {
            let file = candidate.file;
            if file.file_fingerprint != fingerprint
                || file.file_length != size
                || !file
                    .hashes
                    .iter()
                    .any(|hash| hash.algo == 1 && hash.value.eq_ignore_ascii_case(&sha1))
            {
                continue;
            }
            ensure!(
                candidate.id == file.mod_id,
                "CurseForge fingerprint project IDs disagree"
            );
            let result = FileMatch {
                reference: FileRef {
                    project: ProjectRef {
                        source: self.id.clone(),
                        id: ProjectId(file.mod_id.to_string()),
                    },
                    id: FileId(file.id.to_string()),
                },
                filename: file.file_name,
                size,
                hashes: vec![FileHash::Sha1(sha1.clone())],
                role: MatchedFileRole::Primary,
            };
            if matched.as_ref() == Some(&result) {
                continue;
            }
            ensure!(
                matched.is_none(),
                "CurseForge fingerprint identifies multiple files ambiguously"
            );
            matched = Some(result);
        }
        Ok(matched)
    }
}

/// Hash raw bytes for verification and whitespace-filtered bytes for MurmurHash2.
fn fingerprint_file(path: &Path) -> Result<(u64, String, u32)> {
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
        filtered.extend(
            buffer[..count]
                .iter()
                .copied()
                .filter(|b| !matches!(b, 9 | 10 | 13 | 32)),
        );
    }
    Ok((size, format!("{:x}", sha1.finalize()), murmur2(&filtered)?))
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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Matches {
    exact_matches: Vec<Match>,
}

#[derive(Deserialize, Debug)]
struct Match {
    id: u32,
    file: MatchedFile,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct MatchedFile {
    id: u32,
    mod_id: u32,
    file_name: String,
    file_length: u64,
    file_fingerprint: u32,
    hashes: Vec<api::Hash>,
}
