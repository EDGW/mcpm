//! Official upstream version catalog endpoints and tolerant decoders.
//!
//! Sole source of truth for official catalog URLs and raw upstream DTOs across
//! Minecraft, authlib-injector, Forge, NeoForge, and Fabric.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use url::Url;

use super::append_path_segment;
use crate::download::catalog::{
    AuthlibBuildInfo, Catalog, CatalogEndpoint, FabricInstallerEntry, FabricLoaderEntry,
    ForgePromotions, ForgeVersionEntry, MinecraftVersionEntry, MinecraftVersionList,
    NeoForgeVersionEntry,
};

const MOJANG_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const AUTHLIB_LATEST: &str = "https://authlib-injector.yushi.moe/artifact/latest.json";
const AUTHLIB_ARTIFACT_ROOT: &str = "https://authlib-injector.yushi.moe/artifact/";
const FORGE_METADATA: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const FORGE_PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_LEGACY_METADATA: &str =
    "https://maven.neoforged.net/releases/net/neoforged/forge/maven-metadata.xml";
const NEOFORGE_MODERN_METADATA: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
const FABRIC_LOADERS_ROOT: &str = "https://meta.fabricmc.net/v2/versions/loader";
const FABRIC_INSTALLERS: &str = "https://meta.fabricmc.net/v2/versions/installer";

/// Official upstream catalog protocol for all supported download targets.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfficialCatalog;

impl Catalog for OfficialCatalog {
    fn minecraft_versions(&self) -> Result<Option<CatalogEndpoint<MinecraftVersionList>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(MOJANG_MANIFEST)?,
            decode: decode_minecraft_versions,
        }))
    }

    fn authlib_latest(&self) -> Result<Option<CatalogEndpoint<AuthlibBuildInfo>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(AUTHLIB_LATEST)?,
            decode: decode_authlib_build,
        }))
    }

    fn authlib_build(&self, build: u64) -> Result<Option<CatalogEndpoint<AuthlibBuildInfo>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(AUTHLIB_ARTIFACT_ROOT)?.join(&format!("{build}.json"))?,
            decode: decode_authlib_build,
        }))
    }

    fn forge_versions(&self, _mc: &str) -> Result<Option<CatalogEndpoint<Vec<ForgeVersionEntry>>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(FORGE_METADATA)?,
            decode: decode_forge_versions,
        }))
    }

    fn forge_promotions(&self) -> Result<Option<CatalogEndpoint<ForgePromotions>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(FORGE_PROMOTIONS)?,
            decode: decode_forge_promotions,
        }))
    }

    fn neoforge_versions(
        &self,
        mc: &str,
    ) -> Result<Option<CatalogEndpoint<Vec<NeoForgeVersionEntry>>>> {
        if mc == "1.20.1" {
            Ok(Some(CatalogEndpoint {
                url: Url::parse(NEOFORGE_LEGACY_METADATA)?,
                decode: decode_neoforge_legacy_versions,
            }))
        } else {
            Ok(Some(CatalogEndpoint {
                url: Url::parse(NEOFORGE_MODERN_METADATA)?,
                decode: decode_neoforge_modern_versions,
            }))
        }
    }

    fn fabric_loaders(&self, mc: &str) -> Result<Option<CatalogEndpoint<Vec<FabricLoaderEntry>>>> {
        Ok(Some(CatalogEndpoint {
            url: append_path_segment(FABRIC_LOADERS_ROOT, mc, "Minecraft version")?,
            decode: decode_fabric_loaders,
        }))
    }

    fn fabric_installers(&self) -> Result<Option<CatalogEndpoint<Vec<FabricInstallerEntry>>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(FABRIC_INSTALLERS)?,
            decode: decode_fabric_installers,
        }))
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
    let mut entries = Vec::new();
    for raw in parse_maven_versions(bytes, "Forge")? {
        if let Some(entry) = parse_forge_coordinate(&raw) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn parse_forge_coordinate(raw: &str) -> Option<ForgeVersionEntry> {
    if raw
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte == b'/')
    {
        return None;
    }
    let (mc_version, rest) = raw.split_once('-')?;
    if mc_version.is_empty() || rest.is_empty() {
        return None;
    }
    let loader_version = match rest.split_once('-') {
        Some((version, suffix)) if !suffix.is_empty() => version,
        Some(_) => return None,
        None => rest,
    };
    if !is_forge_loader_version(loader_version) {
        return None;
    }
    Some(ForgeVersionEntry {
        mc_version: mc_version.to_owned(),
        loader_version: loader_version.to_owned(),
        maven_coord: raw.to_owned(),
    })
}

fn is_forge_loader_version(version: &str) -> bool {
    let mut count = 0usize;
    for segment in version.split('.') {
        if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        count += 1;
    }
    count >= 3
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

fn decode_neoforge_legacy_versions(bytes: &[u8]) -> Result<Vec<NeoForgeVersionEntry>> {
    let mut entries = Vec::new();
    for raw in parse_maven_versions(bytes, "NeoForge")? {
        if raw
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'/')
        {
            continue;
        }
        let Some(loader_version) = raw.strip_prefix("1.20.1-") else {
            continue;
        };
        if loader_version.is_empty() {
            continue;
        }
        entries.push(NeoForgeVersionEntry {
            mc_version: "1.20.1".to_owned(),
            loader_version: loader_version.to_owned(),
            maven_coord: raw,
        });
    }
    Ok(entries)
}

fn decode_neoforge_modern_versions(bytes: &[u8]) -> Result<Vec<NeoForgeVersionEntry>> {
    let mut entries = Vec::new();
    for raw in parse_maven_versions(bytes, "NeoForge")? {
        if raw
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'/')
        {
            continue;
        }
        let Some(mc_version) = neoforge_mc_version(&raw) else {
            continue;
        };
        entries.push(NeoForgeVersionEntry {
            mc_version,
            loader_version: raw.clone(),
            maven_coord: raw,
        });
    }
    Ok(entries)
}

fn neoforge_mc_version(raw: &str) -> Option<String> {
    let numeric = raw.split(['-', '+']).next()?;
    let mut parts = numeric.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    if major >= 26 {
        let patch = match parts.next() {
            Some(segment) => Some(segment.parse::<u32>().ok()?),
            None => None,
        };
        for segment in parts {
            segment.parse::<u32>().ok()?;
        }
        match patch {
            None | Some(0) => Some(format!("{major}.{minor}")),
            Some(patch) => Some(format!("{major}.{minor}.{patch}")),
        }
    } else if (20..=25).contains(&major) {
        for segment in parts {
            segment.parse::<u32>().ok()?;
        }
        if minor == 0 {
            Some(format!("1.{major}"))
        } else {
            Some(format!("1.{major}.{minor}"))
        }
    } else {
        None
    }
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
