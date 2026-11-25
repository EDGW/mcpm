//! BMCLAPI-specific version catalog overrides for non-official Forge JSON APIs.
//!
//! All other targets stay `Ok(None)` so the downloader translates official URLs
//! through BMCLAPI's static prefix table and reuses official decoders.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use url::Url;

use super::append_path_segment;
use crate::download::catalog::{Catalog, CatalogEndpoint, ForgePromotions, ForgeVersionEntry};

const BMCLAPI_FORGE_MC_ROOT: &str = "https://bmclapi2.bangbang93.com/forge/minecraft";
const BMCLAPI_FORGE_PROMOS: &str = "https://bmclapi2.bangbang93.com/forge/promos";

/// BMCLAPI catalog overrides for Forge's mirror-specific JSON endpoints.
#[derive(Debug, Clone, Copy, Default)]
pub struct BmclapiCatalog;

impl Catalog for BmclapiCatalog {
    fn forge_versions(&self, mc: &str) -> Result<Option<CatalogEndpoint<Vec<ForgeVersionEntry>>>> {
        Ok(Some(CatalogEndpoint {
            url: append_path_segment(BMCLAPI_FORGE_MC_ROOT, mc, "Minecraft version")?,
            decode: decode_forge_versions,
        }))
    }

    fn forge_promotions(&self) -> Result<Option<CatalogEndpoint<ForgePromotions>>> {
        Ok(Some(CatalogEndpoint {
            url: Url::parse(BMCLAPI_FORGE_PROMOS)?,
            decode: decode_forge_promotions,
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
            mc_version: entry.mcversion,
            loader_version: entry.version,
            maven_coord,
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
