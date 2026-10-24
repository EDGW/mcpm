//! Sole codec and schema validation for .identification.json source snapshots.

use super::super::{CacheError, CacheId, error::IoContext, storage};
use crate::source::{FileMatch, FileSummary};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

const VERSION: u32 = 1;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    version: u32,
    blake3: String,
    size: u64,
    pub sha1: String,
    pub sources: BTreeMap<String, FileMatch>,
}
impl Snapshot {
    pub fn new(id: CacheId, summary: &FileSummary) -> Self {
        Self {
            version: VERSION,
            blake3: id.to_string(),
            size: summary.size,
            sha1: summary.sha1.clone(),
            sources: BTreeMap::new(),
        }
    }
    pub fn valid(&self, id: CacheId, size: u64) -> bool {
        let summary = FileSummary {
            size,
            sha1: self.sha1.clone(),
            murmur2: None,
        };
        self.version == VERSION
            && self.blake3 == id.to_string()
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
pub(super) fn read(path: &Path) -> Result<Snapshot, CacheError> {
    let bytes = fs::read(path).at("read source identification", path)?;
    serde_json::from_slice(&bytes).map_err(|source| CacheError::IdentificationData {
        path: path.to_owned(),
        source,
    })
}
pub(super) fn write(path: &Path, value: &Snapshot) -> Result<(), CacheError> {
    let mut bytes = serde_json::to_vec(value).map_err(|source| CacheError::IdentificationData {
        path: path.to_owned(),
        source,
    })?;
    bytes.push(b'\n');
    let mut temp = storage::temporary(path.parent().expect("cache snapshot has a parent"))?;
    temp.write_all(&bytes)
        .at("write source identification", path)?;
    storage::publish(temp, path, true)
}
