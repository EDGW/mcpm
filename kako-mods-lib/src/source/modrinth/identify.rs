//! SHA-1 local file identification preserving the actual matched Modrinth asset.

use anyhow::{Context, Result, ensure};
use reqwest::StatusCode;

use super::{ModrinthSource, api};
use crate::source::{
    FileHash, FileId, FileMatch, FileRef, FileSummary, MatchedFileRole, ProjectId, ProjectRef,
};

impl ModrinthSource {
    pub(super) async fn identify_summary_file(
        &self,
        summary: &FileSummary,
    ) -> Result<Option<FileMatch>> {
        summary.validate()?;
        let started = std::time::Instant::now();
        tracing::debug!(
            size = summary.size,
            "Sending Modrinth identification request using file summary"
        );
        let size = summary.size;
        let hash = &summary.sha1;
        let response = self
            .request(&format!("version_file/{hash}"))?
            .query(&[("algorithm", "sha1")])
            .send()
            .await
            .context("Modrinth file identification failed")?;
        tracing::debug!(status = %response.status(), elapsed_ms = started.elapsed().as_millis(), "Received Modrinth identification response");
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
        let mut hashes = vec![FileHash::Sha1(hash.clone())];
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
