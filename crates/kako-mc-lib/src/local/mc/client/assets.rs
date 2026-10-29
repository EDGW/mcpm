//! Client asset metadata and path resolution.

use std::{
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    manifest::AssetIndex,
    path::{self, ResourcePathError},
};

/// Read-only access to the shared `assets/` object store and index files.
#[derive(Debug, Clone)]
pub struct AssetsDirectory {
    path: PathBuf,
}

impl AssetsDirectory {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn indexes_path(&self) -> PathBuf {
        self.path.join("indexes")
    }
    pub fn objects_path(&self) -> PathBuf {
        self.path.join("objects")
    }

    pub fn log_configs_path(&self) -> PathBuf {
        self.path.join("log_configs")
    }

    pub fn log_config_path(&self, id: &str) -> Result<PathBuf, ResourcePathError> {
        path::within(&self.log_configs_path(), id)
    }

    pub fn index_path(&self, id: &str) -> Result<PathBuf, AssetError> {
        if id.is_empty() || id.contains(['/', '\\', ':', '\0']) || matches!(id, "." | "..") {
            return Err(AssetError::InvalidIndexId { id: id.to_owned() });
        }
        Ok(path::within(&self.indexes_path(), &format!("{id}.json"))?)
    }

    pub fn object_path(&self, hash: &str) -> Result<PathBuf, AssetError> {
        if !path::sha1(hash) {
            return Err(AssetError::InvalidHash {
                hash: hash.to_owned(),
            });
        }
        let hash = hash.to_ascii_lowercase();
        Ok(self.objects_path().join(&hash[..2]).join(hash))
    }

    pub fn read_index(&self, id: &str) -> Result<AssetIndex, AssetError> {
        let path = self.index_path(id)?;
        let bytes = fs::read(&path).map_err(|source| AssetError::ReadIndex {
            path: path.clone(),
            source,
        })?;
        serde_json::from_slice(&bytes).map_err(|source| AssetError::ParseIndex { path, source })
    }
}

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("invalid asset index identifier {id:?}")]
    InvalidIndexId { id: String },
    #[error("invalid asset SHA-1 {hash:?}")]
    InvalidHash { hash: String },
    #[error(transparent)]
    Path(#[from] ResourcePathError),
    #[error("failed to read asset index {path:?}")]
    ReadIndex {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse asset index {path:?}")]
    ParseIndex {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}
