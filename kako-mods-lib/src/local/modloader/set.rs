use std::slice;

use anyhow::Result;
use thiserror::Error;

use super::ModLoaderMeta;

/// An insertion-ordered collection of loaders with unique identifiers.
///
/// IDs are compared exactly and case-sensitively. Removing a loader preserves
/// the relative order of the remaining loaders; new loaders are appended.
#[derive(Default, Clone)]
pub struct ModLoaderSet {
    loaders: Vec<ModLoaderMeta>,
}

impl ModLoaderSet {
    /// Creates an empty loader set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Iterates over loader metadata in insertion order.
    pub fn iter(&self) -> slice::Iter<'_, ModLoaderMeta> {
        self.loaders.iter()
    }

    /// Adds a loader at the end of the set.
    ///
    /// # Errors
    ///
    /// Returns an [`anyhow::Error`] wrapping [`LoaderSetError::DuplicateId`]
    /// if the set already contains the ID. The set is unchanged on failure.
    pub fn add(&mut self, loader: ModLoaderMeta) -> Result<()> {
        if self.loaders.iter().any(|existing| existing.id == loader.id) {
            return Err(LoaderSetError::DuplicateId { id: loader.id }.into());
        }

        self.loaders.push(loader);
        Ok(())
    }

    /// Removes and returns the loader with the supplied ID.
    ///
    /// Returns `None` if the ID is absent. The order of remaining loaders is
    /// preserved; re-adding the removed loader places it at the end.
    pub fn remove(&mut self, id: &str) -> Option<ModLoaderMeta> {
        let index = self.loaders.iter().position(|loader| loader.id == id)?;
        Some(self.loaders.remove(index))
    }
}

/// An error encountered while adding a loader to a [`ModLoaderSet`].
#[derive(Debug, Error)]
pub enum LoaderSetError {
    /// A loader with the same identifier is already present.
    #[error("loader ID {id:?} already exists in the set")]
    DuplicateId {
        /// The identifier that was rejected.
        id: String,
    },
}
