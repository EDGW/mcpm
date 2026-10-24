//! CurseForge fingerprint lookup with independent size and SHA-1 verification.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{CurseForgeSource, api};
use crate::source::{
    FileHash, FileId, FileMatch, FileRef, FileSummary, MatchedFileRole, ProjectId, ProjectRef,
};

impl CurseForgeSource {
    pub(super) async fn identify_summary_file(
        &self,
        summary: &FileSummary,
    ) -> Result<Option<FileMatch>> {
        summary.validate()?;
        let started = std::time::Instant::now();
        tracing::debug!(
            size = summary.size,
            "Sending CurseForge identification request using file summary"
        );
        let size = summary.size;
        let sha1 = &summary.sha1;
        let fingerprint = summary
            .murmur2
            .context("CurseForge identification requires a MurmurHash2 fingerprint")?;
        let response = self
            .request_with_method(reqwest::Method::POST, "fingerprints/432")?
            .json(&serde_json::json!({"fingerprints": [fingerprint]}))
            .send()
            .await
            .context("CurseForge file identification failed")?;
        tracing::debug!(status = %response.status(), elapsed_ms = started.elapsed().as_millis(), "Received CurseForge identification response");
        let response: api::Response<Matches> = response
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
            if matched.is_some() {
                return Ok(None);
            }
            matched = Some(result);
        }
        Ok(matched)
    }
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
