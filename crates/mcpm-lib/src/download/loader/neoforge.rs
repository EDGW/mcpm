//! NeoForge installer version resolution from explicit `MC -> NeoForge` catalogs.
//!
//! Never infers Minecraft compatibility from NeoForge version-string prefixes or parses
//! version numbers; selection relies strictly on catalog-declared `mc_version`, `stable`,
//! and newest-first ordering.

use anyhow::{Context, Result};
use url::Url;

use super::{InstallerRelease, LoaderFamily, finalize_release};
use crate::download::{
    Downloader, Event, append_path_segments,
    catalog::NeoForgeVersionEntry,
    sources::{
        NEOFORGE_LEGACY_MAVEN_ROOT, NEOFORGE_LEGACY_MC, NEOFORGE_MODERN_MAVEN_ROOT, OfficialCatalog,
    },
};

pub(super) async fn resolve(
    downloader: &Downloader,
    mc: &str,
    requested: Option<&str>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<InstallerRelease> {
    let official = OfficialCatalog::neoforge_versions_endpoint()?;
    let (entry, jar_url) = downloader
        .query_catalog_with_progress(
            official,
            |catalog| catalog.neoforge_versions(mc),
            |entries| {
                let entry = match requested {
                    Some(input) => select_requested(entries, mc, input)?,
                    None => select_latest(entries, mc)?,
                };
                let jar_url = installer_jar_url(mc, &entry.maven_coord)?;
                Ok((entry, jar_url))
            },
            progress,
        )
        .await?;

    finalize_release(
        downloader,
        LoaderFamily::NeoForge,
        entry.loader_version,
        jar_url,
        progress,
    )
    .await
}

fn select_requested(
    entries: Vec<NeoForgeVersionEntry>,
    mc: &str,
    input: &str,
) -> Result<NeoForgeVersionEntry> {
    entries
        .into_iter()
        .filter(|entry| entry.mc_version == mc)
        .find(|entry| entry.loader_version == input || entry.maven_coord == input)
        .with_context(|| format!("NeoForge version {input:?} is not available for Minecraft {mc}"))
}

fn select_latest(entries: Vec<NeoForgeVersionEntry>, mc: &str) -> Result<NeoForgeVersionEntry> {
    let mut first = None;
    for entry in entries.into_iter().filter(|entry| entry.mc_version == mc) {
        if entry.stable == Some(true) {
            return Ok(entry);
        }
        if first.is_none() {
            first = Some(entry);
        }
    }
    first.with_context(|| format!("no NeoForge versions available for Minecraft {mc}"))
}

fn installer_jar_url(mc: &str, maven_coord: &str) -> Result<Url> {
    if mc == NEOFORGE_LEGACY_MC {
        append_path_segments(
            NEOFORGE_LEGACY_MAVEN_ROOT,
            &[maven_coord, &format!("forge-{maven_coord}-installer.jar")],
            "NeoForge Maven coordinate",
        )
    } else {
        append_path_segments(
            NEOFORGE_MODERN_MAVEN_ROOT,
            &[maven_coord, &format!("neoforge-{maven_coord}-installer.jar")],
            "NeoForge Maven coordinate",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(mc: &str, loader: &str, coord: &str, stable: Option<bool>) -> NeoForgeVersionEntry {
        NeoForgeVersionEntry {
            mc_version: mc.to_owned(),
            loader_version: loader.to_owned(),
            maven_coord: coord.to_owned(),
            stable,
        }
    }

    fn modern(mc: &str, version: &str, stable: Option<bool>) -> NeoForgeVersionEntry {
        entry(mc, version, version, stable)
    }

    #[test]
    fn isolates_26_1_from_26_1_2_without_version_prefix_guessing() {
        // Catalog order is newest-first; 26.1.2 entries must never leak into MC 26.1.
        let entries = vec![
            modern("26.1.2", "26.1.2.130", Some(true)),
            modern("26.1.2", "26.1.2.129", Some(false)),
            modern("26.1", "26.1.0.19-beta", Some(false)),
            modern("26.1", "26.1.0.18-beta", Some(false)),
        ];

        assert_eq!(
            select_latest(entries.clone(), "26.1").unwrap().loader_version,
            "26.1.0.19-beta"
        );
        assert_eq!(
            select_latest(entries.clone(), "26.1.2")
                .unwrap()
                .loader_version,
            "26.1.2.130"
        );
        assert!(select_requested(entries.clone(), "26.1", "26.1.2.130").is_err());
        assert_eq!(
            select_requested(entries, "26.1", "26.1.0.18-beta")
                .unwrap()
                .maven_coord,
            "26.1.0.18-beta"
        );
    }

    #[test]
    fn legacy_1_20_1_accepts_dual_format_and_uses_forge_maven_group() {
        let entries = vec![
            entry("1.20.1", "47.1.106", "1.20.1-47.1.106", Some(false)),
            entry("1.20.1", "47.1.99", "1.20.1-47.1.99", Some(false)),
            modern("1.20.2", "20.2.88", Some(false)),
        ];

        let short = select_requested(entries.clone(), "1.20.1", "47.1.106").unwrap();
        assert_eq!(short.loader_version, "47.1.106");
        assert_eq!(short.maven_coord, "1.20.1-47.1.106");

        let full = select_requested(entries.clone(), "1.20.1", "1.20.1-47.1.106").unwrap();
        assert_eq!(full.loader_version, "47.1.106");
        assert_eq!(full.maven_coord, "1.20.1-47.1.106");

        let latest = select_latest(entries.clone(), "1.20.1").unwrap();
        assert_eq!(latest.loader_version, "47.1.106");

        assert!(select_requested(entries.clone(), "1.20.1", "20.2.88").is_err());
        assert!(select_requested(entries, "1.20.1", "47.1.999").is_err());

        assert_eq!(
            installer_jar_url("1.20.1", "1.20.1-47.1.106")
                .unwrap()
                .as_str(),
            "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-47.1.106/forge-1.20.1-47.1.106-installer.jar"
        );
    }

    #[test]
    fn latest_prefers_first_stable_falls_back_to_first_and_tolerates_arbitrary_strings() {
        // Prefers first `stable == Some(true)` over earlier `Some(false)`.
        let mixed_stability = vec![
            modern("1.21.1", "21.1.269-beta", Some(false)),
            modern("1.21.1", "21.1.268", Some(true)),
            modern("1.21.1", "21.1.267", Some(true)),
        ];
        assert_eq!(
            select_latest(mixed_stability, "1.21.1")
                .unwrap()
                .loader_version,
            "21.1.268"
        );

        // When all entries are `Some(false)` (current Modrinth manifest) or `None` (BMCLAPI),
        // returns the first (newest) entry without parsing version strings.
        let all_false = vec![
            modern("1.21.1", "custom-latest-tag", Some(false)),
            modern("1.21.1", "21.1.267", Some(false)),
        ];
        assert_eq!(
            select_latest(all_false, "1.21.1").unwrap().loader_version,
            "custom-latest-tag"
        );

        let bmclapi_none = vec![
            modern("1.21.1", "asdfjk", None),
            modern("1.21.1", "21.1.72", None),
        ];
        assert_eq!(
            select_latest(bmclapi_none.clone(), "1.21.1")
                .unwrap()
                .loader_version,
            "asdfjk"
        );
        assert_eq!(
            select_requested(bmclapi_none, "1.21.1", "asdfjk")
                .unwrap()
                .loader_version,
            "asdfjk"
        );

        assert!(select_latest(vec![], "1.21.1").is_err());

        assert_eq!(
            installer_jar_url("1.21.1", "21.1.72").unwrap().as_str(),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.72/neoforge-21.1.72-installer.jar"
        );
    }
}
