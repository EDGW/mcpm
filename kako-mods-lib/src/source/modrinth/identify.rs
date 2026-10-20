//! SHA-1 local file identification preserving the actual matched Modrinth asset.

use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, ensure};
use reqwest::StatusCode;
use sha1::{Digest, Sha1};

use super::{ModrinthSource, api};
use crate::source::{FileHash, FileId, FileMatch, FileRef, MatchedFileRole, ProjectId, ProjectRef};

impl ModrinthSource {
    pub(super) async fn identify_local_file(&self, path: &Path) -> Result<Option<FileMatch>> {
        let path = path.to_owned();
        let (size, hash) = tokio::task::spawn_blocking(move || hash_file(&path))
            .await
            .context("local file hashing task failed")??;
        let response = self
            .request(&format!("version_file/{hash}"))?
            .query(&[("algorithm", "sha1")])
            .send()
            .await
            .context("Modrinth file identification failed")?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let version: api::Version = response
            .error_for_status()?
            .json()
            .await
            .context("invalid Modrinth file identification response")?;
        let mut matches = version.files.into_iter().filter(|file| {
            file.size == size
                && file
                    .hashes
                    .sha1
                    .as_ref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(&hash))
        });
        let Some(file) = matches.next() else {
            anyhow::bail!("Modrinth hash response contains no matching file");
        };
        ensure!(
            matches.next().is_none(),
            "Modrinth hash identifies multiple files ambiguously"
        );
        let mut hashes = vec![FileHash::Sha1(hash)];
        if let Some(hash) = file.hashes.sha512 {
            hashes.push(FileHash::Sha512(hash));
        }
        Ok(Some(FileMatch {
            reference: FileRef {
                project: ProjectRef {
                    source: self.id.clone(),
                    id: ProjectId(version.project_id),
                },
                id: FileId(version.id),
            },
            filename: file.filename,
            size,
            hashes,
            role: if file.primary {
                MatchedFileRole::Primary
            } else {
                MatchedFileRole::Additional
            },
        }))
    }
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open local file {path:?}"))?;
    ensure!(
        file.metadata()?.is_file(),
        "local path must be a regular file"
    );
    let mut hasher = Sha1::new();
    let mut size = 0;
    let mut buffer = [0; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .context("failed to read local file")?;
        if count == 0 {
            break;
        }
        size += count as u64;
        hasher.update(&buffer[..count]);
    }
    Ok((size, format!("{:x}", hasher.finalize())))
}
