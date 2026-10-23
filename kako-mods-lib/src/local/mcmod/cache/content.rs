//! Pinned file observations for hashing and validating content provenance.

use std::{
    fs::{File, Metadata},
    io::{Read, Seek},
    path::{Path, PathBuf},
};

use super::{CacheError, CacheId, error::IoContext, storage};
use crate::local::files::FileIdentity;

/// Keeping the handle alive prevents its native identity from being reused.
pub(super) struct ContentFile {
    pub file: File,
    pub identity: FileIdentity,
    path: PathBuf,
    observed: Metadata,
}

impl ContentFile {
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        let file = storage::open_regular_file(path)?;
        let identity = FileIdentity::from_file(&file).at("read content identity", path)?;
        let observed = file.metadata().at("inspect content handle", path)?;
        let result = Self {
            file,
            identity,
            path: path.to_owned(),
            observed,
        };
        result.validate()?;
        Ok(result)
    }

    /// Detects name replacement and ordinary concurrent content changes.
    /// Cache contents must not be modified in place, including through hard links.
    pub fn validate(&self) -> Result<(), CacheError> {
        let current = self
            .file
            .metadata()
            .at("inspect content handle", &self.path)?;
        if FileIdentity::read(&self.path).at("validate content identity", &self.path)?
            != self.identity
            || current.len() != self.observed.len()
            || current
                .modified()
                .at("inspect content modification time", &self.path)?
                != self
                    .observed
                    .modified()
                    .at("inspect content modification time", &self.path)?
        {
            return Err(CacheError::SourceChanged(self.path.clone()));
        }
        Ok(())
    }

    pub fn id(&mut self) -> Result<CacheId, CacheError> {
        self.file.rewind().at("rewind content", &self.path)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0; 65536];
        loop {
            let count = self.file.read(&mut buffer).at("hash content", &self.path)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        self.validate()?;
        Ok(CacheId::from_hash(hasher.finalize()))
    }

    pub fn quick_hash(&mut self) -> Result<[u8; 16], CacheError> {
        let result =
            super::index::sample::hash(&mut self.file).at("compute imohash", &self.path)?;
        self.validate()?;
        Ok(result)
    }
}
