//! Mod loader identification interfaces and ordered loader collections.

use std::sync::Arc;

use anyhow::Result;
use lazy_static::lazy_static;

use super::mcmod::{ModEntry, ModMeta};

pub mod impls;
mod set;

pub use set::{LoaderSetError, ModLoaderSet};

/// A loader's identity and the instance that provides its behavior.
#[derive(Clone)]
pub struct ModLoaderMeta {
    /// The loader's stable identifier, used to detect duplicates in a loader set.
    pub id: String,
    /// The loader's name.
    pub name: String,
    /// The loader instance associated with this identity.
    pub instance: Arc<dyn ModLoader>,
}

/// A loader that identifies mod files and supplies common identification metadata.
pub trait ModLoader: Send + Sync {
    /// Returns this loader's identity and associated instance.
    ///
    /// No preconditions are required. The returned metadata must describe this
    /// loader. This operation is infallible.
    fn metadata(&self) -> &ModLoaderMeta;

    /// Attempts to identify the mod file referenced by `entry` for this loader.
    ///
    /// `entry` supplies a validated filename, but does not guarantee that its
    /// path exists or contains a readable mod archive. Disabled files can also
    /// be identified; enabled state does not determine loader compatibility.
    ///
    /// Returns `Ok(Some(metadata))` when recognized. The metadata must identify
    /// this loader and include the mod declarations it recognizes in the file,
    /// normalized to the common identification fields. Returns `Ok(None)` when
    /// the archive contains no metadata recognized by this loader. The operation
    /// must leave the file and directory unchanged.
    ///
    /// # Errors
    ///
    /// Returns an [`anyhow::Error`] if the file cannot be read, the archive is
    /// invalid, or this loader's metadata cannot be parsed or normalized.
    /// Malformed recognized metadata must not be reported as `Ok(None)`.
    fn try_identify(&self, entry: &ModEntry) -> Result<Option<ModMeta>>;
}

lazy_static! {
    /// Built-in loaders in identification order: Forge, NeoForge, Fabric, Quilt.
    pub static ref BUILTIN_LOADERS: ModLoaderSet = {
        let mut loaders = ModLoaderSet::new();
        for metadata in [
            impls::forge::ForgeLoader.metadata(),
            impls::neoforge::NeoForgeLoader.metadata(),
            impls::fabric::FabricLoader.metadata(),
            impls::quilt::QuiltLoader.metadata(),
        ] {
            loaders.add(metadata.clone()).expect("built-in loader IDs must be unique");
        }
        loaders
    };
}
