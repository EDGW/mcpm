//! Execution-host file digests for source identification, without network or cache access.

use std::path::PathBuf;

use anyhow::{Context, Result};
use mcpm_exec::operation::Operation;
use mcpm_lib::source::{FileSummary, SummaryRequirements};
use serde::{Deserialize, Serialize};

/// Compute size and SHA-1, plus the requested optional fingerprints, on the file's host.
pub struct Digest;
impl Operation for Digest {
    const ID: &'static str = "mod.digest";
    type Input = DigestInput;
    type Output = FileSummary;

    async fn run(input: DigestInput) -> Result<FileSummary> {
        tokio::task::spawn_blocking(move || {
            FileSummary::read(&input.path, input.requirements)
                .with_context(|| format!("failed to compute Mod file digest at {:?}", input.path))
        })
        .await
        .context("Mod file digest worker failed")?
    }
}

/// A path on the execution host and source-independent digest requirements.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestInput {
    pub path: PathBuf,
    pub requirements: SummaryRequirements,
}
