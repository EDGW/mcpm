//! NeoForge installer version resolution across legacy 1.20.1 and modern 1.20.2+/26.1+ coordinates.

use anyhow::{Context, Result};
use url::Url;

use super::{
    InstallerRelease, LoaderFamily, append_segments, fetch_sha1, parse_numeric_segments,
};
use crate::download::{
    Catalog, DownloadRequest, Downloader, Event, catalog::NeoForgeVersionEntry,
    sources::OfficialCatalog,
};

const NEOFORGE_LEGACY_MC: &str = "1.20.1";
const NEOFORGE_LEGACY_MAVEN_ROOT: &str =
    "https://maven.neoforged.net/releases/net/neoforged/forge";
const NEOFORGE_MODERN_MAVEN_ROOT: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge";

pub(super) async fn resolve(
    downloader: &Downloader,
    mc: &str,
    requested: Option<&str>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<InstallerRelease> {
    let official = OfficialCatalog
        .neoforge_versions(mc)?
        .expect("OfficialCatalog always provides neoforge_versions");
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

    let sha1 = fetch_sha1(downloader, &jar_url, progress).await?;
    let request = DownloadRequest::new(jar_url, None, vec![sha1])?;
    Ok(InstallerRelease::new(
        LoaderFamily::NeoForge,
        entry.loader_version,
        request,
    ))
}

fn select_requested(
    entries: Vec<NeoForgeVersionEntry>,
    mc: &str,
    input: &str,
) -> Result<NeoForgeVersionEntry> {
    let accept_coord = mc == NEOFORGE_LEGACY_MC;
    entries
        .into_iter()
        .find(|entry| {
            entry.mc_version == mc
                && (entry.loader_version == input || (accept_coord && entry.maven_coord == input))
        })
        .with_context(|| format!("NeoForge version {input:?} is not available for Minecraft {mc}"))
}

fn select_latest(entries: Vec<NeoForgeVersionEntry>, mc: &str) -> Result<NeoForgeVersionEntry> {
    let mut best_stable: Option<((Vec<u64>, usize), NeoForgeVersionEntry)> = None;
    let mut best_beta: Option<((Vec<u64>, Vec<u64>, usize), NeoForgeVersionEntry)> = None;

    for (index, entry) in entries.into_iter().enumerate() {
        if entry.mc_version != mc {
            continue;
        }
        let version = entry.loader_version.split('+').next().unwrap_or("");
        match version.split_once('-') {
            None => {
                let Some(nums) = parse_numeric_segments(version) else {
                    continue;
                };
                let key = (nums, index);
                if best_stable.as_ref().is_none_or(|(best, _)| key > *best) {
                    best_stable = Some((key, entry));
                }
            }
            Some((core, prerelease)) => {
                let Some(tail) = parse_beta_tail(prerelease) else {
                    continue;
                };
                let Some(nums) = parse_numeric_segments(core) else {
                    continue;
                };
                let key = (nums, tail, index);
                if best_beta.as_ref().is_none_or(|(best, _)| key > *best) {
                    best_beta = Some((key, entry));
                }
            }
        }
    }

    best_stable
        .map(|(_, entry)| entry)
        .or_else(|| best_beta.map(|(_, entry)| entry))
        .with_context(|| {
            format!("no stable or beta NeoForge versions available for Minecraft {mc}")
        })
}

fn parse_beta_tail(prerelease: &str) -> Option<Vec<u64>> {
    let tail = prerelease.strip_prefix("beta")?;
    if tail.is_empty() {
        return Some(Vec::new());
    }
    let rest = tail.strip_prefix(['.', '-']).unwrap_or(tail);
    if rest.is_empty() {
        return None;
    }
    rest.split(['.', '-'])
        .map(|segment| {
            (!segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| segment.parse::<u64>().ok())
                .flatten()
        })
        .collect()
}

fn installer_jar_url(mc: &str, maven_coord: &str) -> Result<Url> {
    if mc == NEOFORGE_LEGACY_MC {
        append_segments(
            NEOFORGE_LEGACY_MAVEN_ROOT,
            &[maven_coord, &format!("forge-{maven_coord}-installer.jar")],
        )
    } else {
        append_segments(
            NEOFORGE_MODERN_MAVEN_ROOT,
            &[maven_coord, &format!("neoforge-{maven_coord}-installer.jar")],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(mc: &str, loader: &str, coord: &str) -> NeoForgeVersionEntry {
        NeoForgeVersionEntry {
            mc_version: mc.to_owned(),
            loader_version: loader.to_owned(),
            maven_coord: coord.to_owned(),
        }
    }

    #[test]
    fn legacy_1_20_1_accepts_dual_format_and_uses_forge_maven_group() {
        let entries = vec![
            entry("1.20.1", "47.1.99", "1.20.1-47.1.99"),
            entry("1.20.1", "47.1.106", "1.20.1-47.1.106"),
        ];

        let short = select_requested(entries.clone(), "1.20.1", "47.1.106").unwrap();
        assert_eq!(short.loader_version, "47.1.106");
        assert_eq!(short.maven_coord, "1.20.1-47.1.106");

        let full = select_requested(entries.clone(), "1.20.1", "1.20.1-47.1.106").unwrap();
        assert_eq!(full.loader_version, "47.1.106");
        assert_eq!(full.maven_coord, "1.20.1-47.1.106");

        let latest = select_latest(entries.clone(), "1.20.1").unwrap();
        assert_eq!(latest.loader_version, "47.1.106");

        assert!(select_requested(entries, "1.20.1", "1.20.2-48.0.0").is_err());

        assert_eq!(
            installer_jar_url("1.20.1", "1.20.1-47.1.106")
                .unwrap()
                .as_str(),
            "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-47.1.106/forge-1.20.1-47.1.106-installer.jar"
        );
    }

    #[test]
    fn modern_matching_handles_1_21_1_21_1_26_1_and_rejects_mc_mismatch() {
        let entries = vec![
            entry("1.21", "21.0.167", "21.0.167"),
            entry("1.21.1", "21.1.72", "21.1.72"),
            entry("1.21.1", "21.1.73-beta", "21.1.73-beta"),
            entry("26.1", "26.1.0", "26.1.0"),
        ];

        assert_eq!(
            select_requested(entries.clone(), "1.21", "21.0.167")
                .unwrap()
                .loader_version,
            "21.0.167"
        );
        assert_eq!(
            select_requested(entries.clone(), "1.21.1", "21.1.73-beta")
                .unwrap()
                .loader_version,
            "21.1.73-beta"
        );
        assert_eq!(
            select_requested(entries.clone(), "26.1", "26.1.0")
                .unwrap()
                .loader_version,
            "26.1.0"
        );

        // MC version mismatch must be rejected.
        assert!(select_requested(entries.clone(), "1.21.1", "21.0.167").is_err());
        assert!(select_requested(entries, "1.21", "21.1.72").is_err());

        assert_eq!(
            installer_jar_url("1.21.1", "21.1.72").unwrap().as_str(),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.72/neoforge-21.1.72-installer.jar"
        );
    }

    #[test]
    fn latest_prefers_stable_over_beta_and_falls_back_to_highest_beta_not_alpha() {
        let with_stable = vec![
            entry("1.21.1", "21.1.100-beta", "21.1.100-beta"),
            entry("1.21.1", "21.1.9", "21.1.9"),
            entry("1.21.1", "21.1.72", "21.1.72"),
        ];
        assert_eq!(
            select_latest(with_stable, "1.21.1").unwrap().loader_version,
            "21.1.72"
        );

        let beta_only = vec![
            entry("26.1", "26.1.0.0-alpha.99", "26.1.0.0-alpha.99"),
            entry("26.1", "26.1.0.0-beta-alpha.99", "26.1.0.0-beta-alpha.99"),
            entry("26.1", "26.1.0.0-beta-pr.999", "26.1.0.0-beta-pr.999"),
            entry("26.1", "26.1.0.0-beta.", "26.1.0.0-beta."),
            entry("26.1", "26.1.0.0-beta..1", "26.1.0.0-beta..1"),
            entry("26.1", "26.1.0.0-beta.2", "26.1.0.0-beta.2"),
            entry("26.1", "26.1.0.0-beta.10", "26.1.0.0-beta.10"),
            entry("26.1", "26.1.0.0-beta", "26.1.0.0-beta"),
        ];
        assert_eq!(
            select_latest(beta_only, "26.1").unwrap().loader_version,
            "26.1.0.0-beta.10"
        );

        let build_meta = vec![
            entry("1.21.1", "21.1.72+build-1", "21.1.72+build-1"),
            entry("1.21.1", "21.1.100-beta", "21.1.100-beta"),
        ];
        assert_eq!(
            select_latest(build_meta, "1.21.1").unwrap().loader_version,
            "21.1.72+build-1"
        );

        let alpha_only = vec![entry("26.1", "26.1.0.0-alpha.1", "26.1.0.0-alpha.1")];
        assert!(select_latest(alpha_only, "26.1").is_err());
    }
}
