//! Sole JSON codec and schema checks for source identification snapshots.

use crate::{
    local::{cache::hash::Blake3Hash, codec::Codec},
    source::{FileMatch, FileSummary},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const METANAME: &str = "ident";
const VERSION: u32 = 1;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    version: u32,
    blake3: Blake3Hash,
    size: u64,
    pub sha1: String,
    pub sources: BTreeMap<String, FileMatch>,
}
impl Snapshot {
    pub fn new(id: Blake3Hash, summary: &FileSummary) -> Self {
        Self {
            version: VERSION,
            blake3: id,
            size: summary.size,
            sha1: summary.sha1.clone(),
            sources: BTreeMap::new(),
        }
    }
    pub fn valid(&self, id: Blake3Hash, size: u64) -> bool {
        let summary = FileSummary {
            size,
            sha1: self.sha1.clone(),
            murmur2: None,
        };
        self.version == VERSION
            && self.blake3 == id
            && self.size == size
            && summary.validate().is_ok()
            && !self.sources.is_empty()
            && self.sources.iter().all(|(key, file)| {
                !key.is_empty()
                    && !key.contains('@')
                    && file.reference.project.source.0 == *key
                    && file.validate(&summary).is_ok()
            })
    }
}

impl Codec for Snapshot {
    fn decode(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("failed to decode source identification snapshot")
    }
    fn encode(&self) -> Result<Vec<u8>> {
        ensure!(
            self.valid(self.blake3, self.size),
            "invalid source identification snapshot"
        );
        serde_json::to_vec(self).context("failed to encode source identification snapshot")
    }
}
