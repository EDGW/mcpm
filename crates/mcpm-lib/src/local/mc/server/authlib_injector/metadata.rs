//! Sole codec for local installed-agent metadata, distinct from upstream release responses.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    download::{Checksum, ChecksumAlgorithm, authlib_injector::Release},
    local::codec::Codec,
};

/// Metadata for the single current JAR. Its digest always originates from a resolved release.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    format_version: u64,
    pub build_number: u64,
    pub version: String,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    release_time: Option<String>,
}

impl Metadata {
    pub fn from_release(release: &Release) -> Self {
        Self {
            format_version: 1,
            build_number: release.build_number(),
            version: release.version().into(),
            sha256: release.sha256().into(),
            release_time: release.release_time().map(str::to_owned),
        }
    }

    pub fn checksum(&self) -> Result<Checksum> {
        Checksum::new(ChecksumAlgorithm::Sha256, &self.sha256)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.format_version == 1,
            UnsupportedFormat(self.format_version)
        );
        ensure!(
            !self.version.trim().is_empty(),
            "installed authlib-injector version is empty"
        );
        self.checksum()?;
        Ok(())
    }
}

impl Codec for Metadata {
    fn decode(bytes: &[u8]) -> Result<Self> {
        // Inspect format first so a newer schema is never mistaken for repairable corruption.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Header {
            format_version: u64,
        }
        let header: Header =
            serde_json::from_slice(bytes).context("invalid installed authlib-injector metadata")?;
        ensure!(
            header.format_version == 1,
            UnsupportedFormat(header.format_version)
        );
        let value: Self =
            serde_json::from_slice(bytes).context("invalid installed authlib-injector metadata")?;
        value.validate()?;
        Ok(value)
    }

    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .context("failed to encode installed authlib-injector metadata")
    }
}

/// Newer local metadata must not be overwritten by a repair from an older mcpm.
#[derive(Debug, thiserror::Error)]
#[error("unsupported installed authlib-injector metadata format {0}")]
pub(super) struct UnsupportedFormat(pub u64);
