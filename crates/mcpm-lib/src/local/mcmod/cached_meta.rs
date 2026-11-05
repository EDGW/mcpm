//! Portable mod metadata snapshots; runtime loader instances are supplied by callers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{ModInfo, ModMeta, NestedModMeta};
use crate::local::modloader::{LoaderId, ModLoaderMeta};

/// Complete metadata from one supplying loader, without its runtime instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedModMeta {
    pub loader: String,
    pub mods: Vec<ModInfo>,
    pub nested: Vec<CachedNestedModMeta>,
}

impl TryFrom<&ModMeta> for CachedModMeta {
    type Error = MetadataConversionError;

    fn try_from(meta: &ModMeta) -> Result<Self, Self::Error> {
        Self::capture(meta)
    }
}

impl CachedModMeta {
    /// Captures metadata only if every descendant uses its parent's loader.
    pub fn capture(meta: &ModMeta) -> Result<Self, MetadataConversionError> {
        let loader = meta.loader.id().as_str();
        let snapshot = Self {
            loader: loader.to_owned(),
            mods: meta.mods.clone(),
            nested: meta
                .nested
                .iter()
                .map(|child| {
                    if child.meta.loader.id() != meta.loader.id() {
                        return Err(MetadataConversionError::MixedLoaders);
                    }
                    Ok(CachedNestedModMeta {
                        path: child.path.clone(),
                        meta: Self::capture(&child.meta)?,
                    })
                })
                .collect::<Result<_, _>>()?,
        };
        snapshot.validate(loader)?;
        Ok(snapshot)
    }

    /// Restores loader behavior by identity from the caller's registry.
    pub fn to_mod_meta(
        &self,
        loaders: &HashMap<LoaderId, ModLoaderMeta>,
    ) -> Result<ModMeta, MetadataConversionError> {
        self.validate(&self.loader)?;
        let loader = loaders
            .get(self.loader.as_str())
            .ok_or_else(|| MetadataConversionError::UnknownLoader(self.loader.clone()))?;
        Ok(ModMeta {
            loader: loader.clone(),
            mods: self.mods.clone(),
            nested: self
                .nested
                .iter()
                .map(|child| {
                    Ok(NestedModMeta {
                        path: child.path.clone(),
                        meta: child.meta.to_mod_meta(loaders)?,
                    })
                })
                .collect::<Result<_, MetadataConversionError>>()?,
        })
    }

    pub(crate) fn validate(&self, loader: &str) -> Result<(), MetadataConversionError> {
        if loader.is_empty() || self.loader != loader {
            return Err(MetadataConversionError::MixedLoaders);
        }
        let mut seen = std::collections::HashSet::new();
        for child in &self.nested {
            if child.path.is_empty()
                || child.path.starts_with('/')
                || child.path.split('/').any(|part| part == "..")
                || !seen.insert(&child.path)
            {
                return Err(MetadataConversionError::InvalidNestedPath(
                    child.path.clone(),
                ));
            }
            child.meta.validate(loader)?;
        }
        Ok(())
    }
}

/// A nested archive path relative to its immediate parent and its full metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedNestedModMeta {
    pub path: String,
    pub meta: CachedModMeta,
}

#[derive(Debug, Error)]
pub enum MetadataConversionError {
    #[error("metadata tree must use one nonempty loader identity")]
    MixedLoaders,
    #[error("loader {0:?} is not present in the supplied registry")]
    UnknownLoader(String),
    #[error("invalid or repeated nested archive member path {0:?}")]
    InvalidNestedPath(String),
}
