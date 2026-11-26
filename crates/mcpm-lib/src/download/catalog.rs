//! Pure descriptive version catalog contracts and normalized target list models.
//!
//! Catalog implementations describe upstream URLs and pure byte decoders without
//! performing HTTP I/O or depending on concrete transport behavior.

use std::{collections::HashMap, fmt};

use anyhow::Result;
use chrono::{DateTime, FixedOffset};
use url::Url;

/// Optional mirror-specific version catalog protocol.
///
/// Returning `Ok(None)` delegates to the official catalog protocol translated
/// through [`DownloadSource::translate`](super::DownloadSource::translate).
pub trait Catalog: Send + Sync {
    fn minecraft_versions(&self) -> Result<Option<CatalogEndpoint<MinecraftVersionList>>> {
        Ok(None)
    }

    fn authlib_latest(&self) -> Result<Option<CatalogEndpoint<AuthlibBuildInfo>>> {
        Ok(None)
    }

    fn authlib_build(&self, _build: u64) -> Result<Option<CatalogEndpoint<AuthlibBuildInfo>>> {
        Ok(None)
    }

    fn forge_versions(&self, _mc: &str) -> Result<Option<CatalogEndpoint<Vec<ForgeVersionEntry>>>> {
        Ok(None)
    }

    fn forge_promotions(&self) -> Result<Option<CatalogEndpoint<ForgePromotions>>> {
        Ok(None)
    }

    fn neoforge_versions(
        &self,
        _mc: &str,
    ) -> Result<Option<CatalogEndpoint<Vec<NeoForgeVersionEntry>>>> {
        Ok(None)
    }

    fn fabric_loaders(&self, _mc: &str) -> Result<Option<CatalogEndpoint<Vec<FabricLoaderEntry>>>> {
        Ok(None)
    }

    fn fabric_installers(&self) -> Result<Option<CatalogEndpoint<Vec<FabricInstallerEntry>>>> {
        Ok(None)
    }
}

/// Pure description of one catalog query: target URL and pure byte decoder.
pub struct CatalogEndpoint<T> {
    pub url: Url,
    pub decode: fn(&[u8]) -> Result<T>,
}

impl<T> Clone for CatalogEndpoint<T> {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            decode: self.decode,
        }
    }
}

impl<T> fmt::Debug for CatalogEndpoint<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CatalogEndpoint")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// Normalized Minecraft version manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftVersionList {
    pub latest_release: String,
    pub latest_snapshot: String,
    pub entries: Vec<MinecraftVersionEntry>,
}

/// One entry in the normalized Minecraft version manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftVersionEntry {
    pub id: String,
    pub url: Url,
    pub sha1: Option<String>,
    pub release_time: DateTime<FixedOffset>,
}

/// Normalized authlib-injector build descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthlibBuildInfo {
    pub build_number: u64,
    pub version: String,
    pub release_time: Option<String>,
    pub download_url: Url,
    pub sha256: String,
}

/// One normalized Forge installer entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeVersionEntry {
    pub maven_coord: String,
    /// Populated when mirror catalogs (e.g. BMCLAPI) supply structured fields;
    /// `None` for raw Official Maven metadata, resolved against target `mc` by `loader::forge`.
    pub mc_version: Option<String>,
    pub loader_version: Option<String>,
}

/// Normalized Forge promoted versions keyed by Minecraft version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgePromotions {
    pub recommended: HashMap<String, String>,
    pub latest: HashMap<String, String>,
}

/// One normalized NeoForge installer entry.
///
/// Catalog decoders normalize entries into newest-first order per Minecraft version so
/// selection logic never parses version numbers or depends on upstream ordering differences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeoForgeVersionEntry {
    pub mc_version: String,
    pub loader_version: String,
    pub maven_coord: String,
    /// `Some(stable)` when supplied by the catalog (e.g. Modrinth launcher-meta),
    /// or `None` when omitted (e.g. BMCLAPI).
    pub stable: Option<bool>,
}

/// One normalized Fabric loader version entry compatible with a Minecraft version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricLoaderEntry {
    pub version: String,
    pub stable: bool,
}

/// One normalized Fabric installer version entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricInstallerEntry {
    pub version: String,
    pub maven_coord: String,
    pub stable: bool,
}
