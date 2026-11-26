//! Official upstream version catalog endpoints and tolerant decoders.
//!
//! Sole source of truth for official catalog URLs and raw upstream DTOs across
//! Minecraft, authlib-injector, Forge, NeoForge, and Fabric.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use url::Url;

use crate::download::{
    append_path_segments,
    catalog::{
        AuthlibBuildInfo, Catalog, CatalogEndpoint, FabricInstallerEntry, FabricLoaderEntry,
        ForgePromotions, ForgeVersionEntry, MinecraftVersionEntry, MinecraftVersionList,
        NeoForgeVersionEntry,
    },
};

pub(crate) const FORGE_MAVEN_ROOT: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge";
pub(crate) const NEOFORGE_LEGACY_MC: &str = "1.20.1";
pub(crate) const NEOFORGE_LEGACY_MAVEN_ROOT: &str =
    "https://maven.neoforged.net/releases/net/neoforged/forge";
pub(crate) const NEOFORGE_MODERN_MAVEN_ROOT: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge";
pub(crate) const FABRIC_MAVEN_ROOT: &str = "https://maven.fabricmc.net/";

const MOJANG_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const AUTHLIB_LATEST: &str = "https://authlib-injector.yushi.moe/artifact/latest.json";
const AUTHLIB_ARTIFACT_ROOT: &str = "https://authlib-injector.yushi.moe/artifact/";
const FORGE_PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_MANIFEST: &str = "https://launcher-meta.modrinth.com/neo/v0/manifest.json";
const FABRIC_LOADERS_ROOT: &str = "https://meta.fabricmc.net/v2/versions/loader";
const FABRIC_INSTALLERS: &str = "https://meta.fabricmc.net/v2/versions/installer";

/// Official upstream catalog protocol for all supported download targets.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfficialCatalog;

impl OfficialCatalog {
    pub fn minecraft_versions_endpoint() -> Result<CatalogEndpoint<MinecraftVersionList>> {
        Ok(CatalogEndpoint {
            url: Url::parse(MOJANG_MANIFEST)?,
            decode: decode_minecraft_versions,
        })
    }

    pub fn authlib_latest_endpoint() -> Result<CatalogEndpoint<AuthlibBuildInfo>> {
        Ok(CatalogEndpoint {
            url: Url::parse(AUTHLIB_LATEST)?,
            decode: decode_authlib_build,
        })
    }

    pub fn authlib_build_endpoint(build: u64) -> Result<CatalogEndpoint<AuthlibBuildInfo>> {
        Ok(CatalogEndpoint {
            url: Url::parse(AUTHLIB_ARTIFACT_ROOT)?.join(&format!("{build}.json"))?,
            decode: decode_authlib_build,
        })
    }

    pub fn forge_versions_endpoint() -> Result<CatalogEndpoint<Vec<ForgeVersionEntry>>> {
        Ok(CatalogEndpoint {
            url: append_path_segments(FORGE_MAVEN_ROOT, &["maven-metadata.xml"], "Forge metadata")?,
            decode: decode_forge_versions,
        })
    }

    pub fn forge_promotions_endpoint() -> Result<CatalogEndpoint<ForgePromotions>> {
        Ok(CatalogEndpoint {
            url: Url::parse(FORGE_PROMOTIONS)?,
            decode: decode_forge_promotions,
        })
    }

    pub fn neoforge_versions_endpoint() -> Result<CatalogEndpoint<Vec<NeoForgeVersionEntry>>> {
        Ok(CatalogEndpoint {
            url: Url::parse(NEOFORGE_MANIFEST)?,
            decode: decode_neoforge_versions,
        })
    }

    pub fn fabric_loaders_endpoint(mc: &str) -> Result<CatalogEndpoint<Vec<FabricLoaderEntry>>> {
        Ok(CatalogEndpoint {
            url: append_path_segments(FABRIC_LOADERS_ROOT, &[mc], "Minecraft version")?,
            decode: decode_fabric_loaders,
        })
    }

    pub fn fabric_installers_endpoint() -> Result<CatalogEndpoint<Vec<FabricInstallerEntry>>> {
        Ok(CatalogEndpoint {
            url: Url::parse(FABRIC_INSTALLERS)?,
            decode: decode_fabric_installers,
        })
    }
}

impl Catalog for OfficialCatalog {
    fn minecraft_versions(&self) -> Result<Option<CatalogEndpoint<MinecraftVersionList>>> {
        Ok(Some(Self::minecraft_versions_endpoint()?))
    }

    fn authlib_latest(&self) -> Result<Option<CatalogEndpoint<AuthlibBuildInfo>>> {
        Ok(Some(Self::authlib_latest_endpoint()?))
    }

    fn authlib_build(&self, build: u64) -> Result<Option<CatalogEndpoint<AuthlibBuildInfo>>> {
        Ok(Some(Self::authlib_build_endpoint(build)?))
    }

    fn forge_versions(&self, _mc: &str) -> Result<Option<CatalogEndpoint<Vec<ForgeVersionEntry>>>> {
        Ok(Some(Self::forge_versions_endpoint()?))
    }

    fn forge_promotions(&self) -> Result<Option<CatalogEndpoint<ForgePromotions>>> {
        Ok(Some(Self::forge_promotions_endpoint()?))
    }

    fn neoforge_versions(
        &self,
        _mc: &str,
    ) -> Result<Option<CatalogEndpoint<Vec<NeoForgeVersionEntry>>>> {
        Ok(Some(Self::neoforge_versions_endpoint()?))
    }

    fn fabric_loaders(&self, mc: &str) -> Result<Option<CatalogEndpoint<Vec<FabricLoaderEntry>>>> {
        Ok(Some(Self::fabric_loaders_endpoint(mc)?))
    }

    fn fabric_installers(&self) -> Result<Option<CatalogEndpoint<Vec<FabricInstallerEntry>>>> {
        Ok(Some(Self::fabric_installers_endpoint()?))
    }
}

fn decode_minecraft_versions(bytes: &[u8]) -> Result<MinecraftVersionList> {
    let manifest: MojangManifest =
        serde_json::from_slice(bytes).context("invalid Mojang version catalog")?;
    Ok(MinecraftVersionList {
        latest_release: manifest.latest.release,
        latest_snapshot: manifest.latest.snapshot,
        entries: manifest
            .versions
            .into_iter()
            .map(|entry| MinecraftVersionEntry {
                id: entry.id,
                url: entry.url,
                sha1: entry.sha1,
                release_time: entry.release_time,
            })
            .collect(),
    })
}

fn decode_authlib_build(bytes: &[u8]) -> Result<AuthlibBuildInfo> {
    let metadata: AuthlibMetadata =
        serde_json::from_slice(bytes).context("invalid authlib-injector release JSON")?;
    Ok(AuthlibBuildInfo {
        build_number: metadata.build_number,
        version: metadata.version,
        release_time: metadata.release_time,
        download_url: metadata.download_url,
        sha256: metadata.checksums.sha256,
    })
}

fn decode_forge_versions(bytes: &[u8]) -> Result<Vec<ForgeVersionEntry>> {
    Ok(parse_maven_versions(bytes, "Forge")?
        .into_iter()
        .filter(|raw| !raw.contains(['/', '\\']))
        .map(|maven_coord| ForgeVersionEntry {
            maven_coord,
            mc_version: None,
            loader_version: None,
        })
        .collect())
}

fn decode_forge_promotions(bytes: &[u8]) -> Result<ForgePromotions> {
    let document: ForgePromotionsDoc =
        serde_json::from_slice(bytes).context("invalid Forge promotions_slim.json")?;
    let mut recommended = HashMap::new();
    let mut latest = HashMap::new();
    for (key, version) in document.promos {
        if version.trim().is_empty() {
            continue;
        }
        if let Some(mc) = key.strip_suffix("-recommended")
            && !mc.is_empty()
        {
            recommended.insert(mc.to_owned(), version);
        } else if let Some(mc) = key.strip_suffix("-latest")
            && !mc.is_empty()
        {
            latest.insert(mc.to_owned(), version);
        }
    }
    Ok(ForgePromotions {
        recommended,
        latest,
    })
}

/// Normalizes a NeoForge version token into `(loader_version, maven_coord)` without
/// assuming any prefix relationship between NeoForge and Minecraft versions.
///
/// Only the historical `1.20.1` (`net.neoforged:forge`) transition uses `1.20.1-<loader>`
/// as its Maven coordinate while exposing `<loader>` as its canonical `loader_version`.
pub(super) fn normalize_neoforge_version(mc: &str, raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains(['/', '\\']) {
        return None;
    }
    if mc == NEOFORGE_LEGACY_MC {
        let prefix = format!("{NEOFORGE_LEGACY_MC}-");
        if let Some(short) = trimmed.strip_prefix(&prefix) {
            if short.is_empty() {
                return None;
            }
            Some((short.to_owned(), trimmed.to_owned()))
        } else {
            Some((trimmed.to_owned(), format!("{prefix}{trimmed}")))
        }
    } else {
        Some((trimmed.to_owned(), trimmed.to_owned()))
    }
}

fn decode_neoforge_versions(bytes: &[u8]) -> Result<Vec<NeoForgeVersionEntry>> {
    let manifest: ModrinthNeoManifest =
        serde_json::from_slice(bytes).context("invalid Modrinth NeoForge version catalog")?;
    let mut normalized = Vec::new();
    for game_version in manifest.game_versions {
        let mc_version = game_version.id.trim();
        if mc_version.is_empty() {
            continue;
        }
        for loader in game_version.loaders {
            let Some((loader_version, maven_coord)) =
                normalize_neoforge_version(mc_version, &loader.id)
            else {
                continue;
            };
            normalized.push(NeoForgeVersionEntry {
                mc_version: mc_version.to_owned(),
                loader_version,
                maven_coord,
                stable: Some(loader.stable),
            });
        }
    }
    Ok(normalized)
}

fn parse_maven_versions(bytes: &[u8], label: &str) -> Result<Vec<String>> {
    let xml = std::str::from_utf8(bytes)
        .with_context(|| format!("{label} maven-metadata.xml is not valid UTF-8"))?;
    let document = roxmltree::Document::parse(xml)
        .with_context(|| format!("invalid {label} maven-metadata.xml"))?;
    let versions = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "versions")
        .with_context(|| format!("missing <versions> in {label} maven-metadata.xml"))?;
    Ok(versions
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "version")
        .filter_map(|node| node.text().map(str::trim))
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .collect())
}

fn decode_fabric_loaders(bytes: &[u8]) -> Result<Vec<FabricLoaderEntry>> {
    let records: Vec<FabricLoaderRecord> =
        serde_json::from_slice(bytes).context("invalid Fabric loader catalog")?;
    Ok(records
        .into_iter()
        .filter(|record| !record.loader.version.trim().is_empty())
        .map(|record| FabricLoaderEntry {
            version: record.loader.version,
            stable: record.loader.stable,
        })
        .collect())
}

fn decode_fabric_installers(bytes: &[u8]) -> Result<Vec<FabricInstallerEntry>> {
    let records: Vec<FabricInstallerRecord> =
        serde_json::from_slice(bytes).context("invalid Fabric installer catalog")?;
    Ok(records
        .into_iter()
        .filter(|record| !record.version.trim().is_empty() && !record.maven.trim().is_empty())
        .map(|record| FabricInstallerEntry {
            version: record.version,
            maven_coord: record.maven,
            stable: record.stable,
        })
        .collect())
}

#[derive(Deserialize)]
struct MojangManifest {
    latest: MojangLatest,
    versions: Vec<MojangVersion>,
}

#[derive(Deserialize)]
struct MojangLatest {
    release: String,
    snapshot: String,
}

#[derive(Deserialize)]
struct MojangVersion {
    id: String,
    url: Url,
    sha1: Option<String>,
    #[serde(rename = "releaseTime")]
    release_time: DateTime<FixedOffset>,
}

#[derive(Deserialize)]
struct AuthlibMetadata {
    build_number: u64,
    version: String,
    #[serde(default)]
    release_time: Option<String>,
    download_url: Url,
    checksums: AuthlibChecksums,
}

#[derive(Deserialize)]
struct AuthlibChecksums {
    sha256: String,
}

#[derive(Deserialize)]
struct ForgePromotionsDoc {
    promos: HashMap<String, String>,
}

/// Strictly ignores `url` fields from Modrinth's NeoForge launcher-meta manifest.
#[derive(Deserialize)]
struct ModrinthNeoManifest {
    #[serde(rename = "gameVersions")]
    game_versions: Vec<ModrinthNeoGameVersion>,
}

#[derive(Deserialize)]
struct ModrinthNeoGameVersion {
    id: String,
    loaders: Vec<ModrinthNeoLoader>,
}

#[derive(Deserialize)]
struct ModrinthNeoLoader {
    id: String,
    stable: bool,
}

#[derive(Deserialize)]
struct FabricLoaderRecord {
    loader: FabricLoaderMeta,
}

#[derive(Deserialize)]
struct FabricLoaderMeta {
    version: String,
    stable: bool,
}

#[derive(Deserialize)]
struct FabricInstallerRecord {
    version: String,
    maven: String,
    stable: bool,
}
