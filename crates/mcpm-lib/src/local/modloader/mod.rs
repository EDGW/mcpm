//! Mod loader identification interfaces and management of the complete loader registry.
//!
//! This module defines the interfaces and manages loader implementations, so it
//! assembles [`BUILTIN_LOADERS`] here. Every loader must be registered in that
//! collection; behavior for unregistered loaders is undefined by the library's
//! API contract.

use std::{collections::HashMap, fmt::Debug};

use anyhow::Result;
use lazy_static::lazy_static;

use super::mcmod::{
    ModEntry, ModInfo, ModMeta,
    dependency::{Dependency, DependencyMatch},
};

mod id;
pub mod impls;
mod meta;

pub use id::LoaderId;
pub use meta::ModLoaderMeta;

/// A recognized loader and its declared version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderInfo {
    pub loader: ModLoaderMeta,
    pub version: String,
}

/// Version evidence returned by one server loader implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerLoaderIdentity {
    pub minecraft_version: String,
    pub version: String,
}

/// A loader that identifies mod files and supplies common identification metadata.
pub trait ModLoader: Send + Sync + Debug {
    /// Returns this loader's identity and associated instance.
    ///
    /// No preconditions are required. The returned metadata must describe this
    /// loader. This operation is infallible.
    fn metadata(&self) -> &ModLoaderMeta;

    /// Tests a candidate against the identity and version conditions of a
    /// dependency supplied by this loader, using this loader's matching rules.
    ///
    /// The candidate must be interpreted in this loader's context. This pure
    /// predicate performs no I/O and does not evaluate relation kind, physical
    /// side, loading order, or whether the complete mod set can load. A matching
    /// incompatibility declaration therefore returns `Matched`, not `Unmatched`.
    /// Return `Unknown` when information is insufficient or evaluation is not
    /// supported; do not treat an unrecognized version format as a mismatch.
    /// The default reports unsupported evaluation.
    fn matches_dependency(
        &self,
        _candidate: &ModInfo,
        _dependency: &Dependency,
    ) -> DependencyMatch {
        DependencyMatch::Unknown
    }

    /// Verifies a statically parsed Java server entry using local evidence.
    /// Unrecognized entries return `None`; missing or conflicting identity evidence
    /// returns an implementation-specific error. Command validity and launchability
    /// are not checked. No execution or downloads.
    fn try_identify_server(
        &self,
        _launch: &super::mc::server::ServerLaunch,
    ) -> Result<Option<ServerLoaderIdentity>> {
        Ok(None)
    }

    /// Identifies this loader in client profile declarations and returns its
    /// loader version according to its recognition rules.
    /// The manifest may already have inherited parent libraries. This operation
    /// performs no I/O. `None` means unrecognized, including loaders that do not
    /// support client inspection. Malformed recognized declarations must return
    /// an error defined by the implementation, not `None`.
    /// Built-in detectors follow HMCL: later library
    /// matches replace earlier ones; NeoForge prefers versions in game arguments.
    /// Visible HMCL patch versions are applied by the client aggregation API.
    fn try_identify_client(
        &self,
        _manifest: &super::mc::client::MinecraftVersionManifest,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    /// Attempts to identify the mod file referenced by `entry` for this loader.
    ///
    /// `entry` supplies a validated filename, but does not guarantee that its
    /// path exists or contains a readable mod archive. Disabled files can also
    /// be identified; enabled state does not determine loader compatibility.
    ///
    /// Returns `Ok(Some(metadata))` when recognized. The metadata must identify
    /// this loader and include the mod declarations it recognizes in the file,
    /// with direct declarations in `mods` and declared child JARs in `nested`.
    /// Child paths are relative to their immediate parent JAR and every node
    /// retains this supplying loader. A wrapper with no direct mods can still
    /// be recognized through its descendants. Returns `Ok(None)` when
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
    /// The complete set of all loaders, indexed by their stable identifiers.
    ///
    /// Every loader must be registered here. Loader maps passed to identification
    /// APIs select from this set; they do not extend it. Behavior for loaders
    /// outside this registry is undefined by the library's API contract.
    pub static ref BUILTIN_LOADERS: HashMap<LoaderId, ModLoaderMeta> = [
        impls::fml::forge::ForgeLoader.metadata(),
        impls::fml::neoforge::NeoForgeLoader.metadata(),
        impls::fabric::FabricLoader.metadata(),
    ]
    .into_iter()
    .map(|metadata| (metadata.id().clone(), metadata.clone()))
    .collect();
}
