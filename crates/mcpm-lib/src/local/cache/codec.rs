//! JSON codec and version validation for generic cache directory metadata.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::local::codec::Codec;

pub const CACHE_DIRECTORY_VERSION: usize = 3;

/// Versioned metadata describing a cache directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheDirMetadata {
    pub version: usize,
}

impl CacheDirMetadata {
    /// Creates metadata for the current cache directory format.
    pub fn new() -> Self {
        Self {
            version: CACHE_DIRECTORY_VERSION,
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == CACHE_DIRECTORY_VERSION,
            "unsupported cache directory metadata version {}; expected {CACHE_DIRECTORY_VERSION}",
            self.version
        );
        Ok(())
    }
}

impl Codec for CacheDirMetadata {
    /// Decodes JSON metadata, rejecting malformed or unsupported formats.
    fn decode(data: &[u8]) -> Result<Self> {
        let metadata: Self =
            serde_json::from_slice(data).context("failed to decode cache directory metadata")?;
        metadata.validate()?;
        Ok(metadata)
    }

    /// Encodes supported metadata as compact JSON.
    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec(self).context("failed to encode cache directory metadata")
    }
}

impl Default for CacheDirMetadata {
    fn default() -> Self {
        Self::new()
    }
}
