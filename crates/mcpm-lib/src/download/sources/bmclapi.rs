//! BMCLAPI-specific version catalog overrides for non-official Forge and NeoForge JSON APIs.
//!
//! All other targets stay `Ok(None)` so the downloader translates official URLs
//! through BMCLAPI's static prefix table and reuses official decoders.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use url::Url;

use super::official::normalize_neoforge_version;
use crate::download::{
    append_path_segments,
    catalog::{
        Catalog, CatalogEndpoint, ForgePromotions, ForgeVersionEntry, NeoForgeVersionEntry,
    },
};

const BMCLAPI_FORGE_MC_ROOT: &str = "https://bmclapi2.bangbang93.com/forge/minecraft";
const BMCLAPI_FORGE_PROMOS: &str = "https://bmclapi2.bangbang93.com/forge/promos";
const BMCLAPI_NEOFORGE_LIST_ROOT: &str = "https://bmclapi2.bangbang93.com/neoforge/list";

/// BMCLAPI catalog overrides for Forge and NeoForge mirror-specific JSON endpoints.
#[derive(Debug, Clone, Copy, Default)]
pub struct BmclapiCatalog;

impl Catalog for BmclapiCatalog {
    fn forge_versions(&self, mc: &str) -> Result<Option<CatalogEndpoint<Vec<ForgeVersionEntry>>>> {
        Ok(Some(CatalogEndpoint {
            url: append_path_segments(BMCLAPI_FORGE_MC_ROOT, &[mc], "Minecraft version")?,
            decode: decode_forge_versions,
        }))
    }

    fn forge_promotions(&self) -> Result<Option<CatalogEndpoint<ForgePromotions>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(BMCLAPI_FORGE_PROMOS)?,
            decode: decode_forge_promotions,
        }))
    }

    fn neoforge_versions(
        &self,
        mc: &str,
    ) -> Result<Option<CatalogEndpoint<Vec<NeoForgeVersionEntry>>>> {
        Ok(Some(CatalogEndpoint {
            url: append_path_segments(BMCLAPI_NEOFORGE_LIST_ROOT, &[mc], "Minecraft version")?,
            decode: decode_neoforge_versions,
        }))
    }
}

fn decode_forge_versions(bytes: &[u8]) -> Result<Vec<ForgeVersionEntry>> {
    let entries: Vec<BmclapiForgeEntry> =
        serde_json::from_slice(bytes).context("invalid BMCLAPI Forge version list")?;
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.mcversion.trim().is_empty() || entry.version.trim().is_empty() {
            continue;
        }
        let maven_coord = match entry
            .branch
            .as_deref()
            .filter(|branch| !branch.trim().is_empty())
        {
            Some(branch) => format!("{}-{}-{branch}", entry.mcversion, entry.version),
            None => format!("{}-{}", entry.mcversion, entry.version),
        };
        normalized.push(ForgeVersionEntry {
            maven_coord,
            mc_version: Some(entry.mcversion),
            loader_version: Some(entry.version),
        });
    }
    Ok(normalized)
}

fn decode_forge_promotions(bytes: &[u8]) -> Result<ForgePromotions> {
    let entries: Vec<BmclapiForgePromo> =
        serde_json::from_slice(bytes).context("invalid BMCLAPI Forge promotions list")?;
    let mut recommended = HashMap::new();
    let mut latest = HashMap::new();
    for entry in entries {
        let Some(build) = entry.build else {
            continue;
        };
        if build.version.trim().is_empty() {
            continue;
        }
        if let Some(mc) = entry.name.strip_suffix("-recommended")
            && !mc.is_empty()
        {
            recommended.insert(mc.to_owned(), build.version);
        } else if let Some(mc) = entry.name.strip_suffix("-latest")
            && !mc.is_empty()
        {
            latest.insert(mc.to_owned(), build.version);
        }
    }
    Ok(ForgePromotions {
        recommended,
        latest,
    })
}

fn decode_neoforge_versions(bytes: &[u8]) -> Result<Vec<NeoForgeVersionEntry>> {
    let entries: Vec<BmclapiNeoForgeEntry> =
        serde_json::from_slice(bytes).context("invalid BMCLAPI NeoForge version list")?;
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        let mc_version = entry.mcversion.trim();
        if mc_version.is_empty() {
            continue;
        }
        let candidate = entry
            .version
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                entry.raw_version.as_deref().and_then(|raw| {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    let forge_prefix = format!("{mc_version}-forge-");
                    Some(
                        trimmed
                            .strip_prefix("neoforge-")
                            .or_else(|| trimmed.strip_prefix(&forge_prefix))
                            .unwrap_or(trimmed),
                    )
                })
            });
        let Some(raw) = candidate else {
            continue;
        };
        let Some((loader_version, maven_coord)) = normalize_neoforge_version(mc_version, raw)
        else {
            continue;
        };
        normalized.push(NeoForgeVersionEntry {
            mc_version: mc_version.to_owned(),
            loader_version,
            maven_coord,
            stable: None,
        });
    }
    // BMCLAPI returns entries oldest-first; reverse so all catalogs share newest-first order.
    normalized.reverse();
    Ok(normalized)
}

#[derive(Deserialize)]
struct BmclapiForgeEntry {
    mcversion: String,
    version: String,
    #[serde(default)]
    branch: Option<String>,
}

#[derive(Deserialize)]
struct BmclapiForgePromo {
    name: String,
    #[serde(default)]
    build: Option<BmclapiForgePromoBuild>,
}

#[derive(Deserialize)]
struct BmclapiForgePromoBuild {
    version: String,
}

#[derive(Deserialize)]
struct BmclapiNeoForgeEntry {
    mcversion: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "rawVersion")]
    raw_version: Option<String>,
}
