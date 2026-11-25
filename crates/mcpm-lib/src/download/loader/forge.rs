//! Forge installer version resolution, dual-format matching, and official Maven URL construction.

use anyhow::{Context, Result};
use url::Url;

use super::{
    InstallerRelease, LoaderFamily, append_segments, fetch_sha1, parse_numeric_segments,
};
use crate::download::{
    Catalog, DownloadRequest, Downloader, Event,
    catalog::{ForgePromotions, ForgeVersionEntry},
    sources::OfficialCatalog,
};

const FORGE_MAVEN_ROOT: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";

pub(super) async fn resolve(
    downloader: &Downloader,
    mc: &str,
    requested: Option<&str>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<InstallerRelease> {
    let target = match requested {
        Some(input) => Some(input.to_owned()),
        None => {
            let promos_official = OfficialCatalog
                .forge_promotions()?
                .expect("OfficialCatalog always provides forge_promotions");
            downloader
                .query_catalog_with_progress(
                    promos_official,
                    |catalog| catalog.forge_promotions(),
                    |promos| Ok(select_promoted(&promos, mc).map(str::to_owned)),
                    progress,
                )
                .await?
        }
    };

    let versions_official = OfficialCatalog
        .forge_versions(mc)?
        .expect("OfficialCatalog always provides forge_versions");
    let (entry, jar_url) = downloader
        .query_catalog_with_progress(
            versions_official,
            |catalog| catalog.forge_versions(mc),
            |entries| {
                let entry = match target.as_deref() {
                    Some(version) => select_requested(entries, mc, version)?,
                    None => select_latest(entries, mc)?,
                };
                let jar_url = installer_jar_url(&entry.maven_coord)?;
                Ok((entry, jar_url))
            },
            progress,
        )
        .await?;

    let sha1 = fetch_sha1(downloader, &jar_url, progress).await?;
    let request = DownloadRequest::new(jar_url, None, vec![sha1])?;
    Ok(InstallerRelease::new(
        LoaderFamily::Forge,
        entry.loader_version,
        request,
    ))
}

fn select_promoted<'a>(promos: &'a ForgePromotions, mc: &str) -> Option<&'a str> {
    let non_empty = |map: &'a std::collections::HashMap<String, String>| {
        map.get(mc)
            .map(|version| version.trim())
            .filter(|version| !version.is_empty())
    };
    non_empty(&promos.recommended).or_else(|| non_empty(&promos.latest))
}

fn select_requested(
    entries: Vec<ForgeVersionEntry>,
    mc: &str,
    input: &str,
) -> Result<ForgeVersionEntry> {
    let matches: Vec<_> = entries
        .into_iter()
        .filter(|entry| {
            entry.mc_version == mc && (entry.loader_version == input || entry.maven_coord == input)
        })
        .collect();
    pick_preferred_entry(matches, mc)
        .with_context(|| format!("Forge version {input:?} is not available for Minecraft {mc}"))
}

fn select_latest(entries: Vec<ForgeVersionEntry>, mc: &str) -> Result<ForgeVersionEntry> {
    let mut best_key: Option<Vec<u64>> = None;
    let mut best_entries = Vec::new();
    for entry in entries {
        if entry.mc_version != mc {
            continue;
        }
        let Some(key) = parse_numeric_segments(&entry.loader_version) else {
            continue;
        };
        match &best_key {
            Some(current) if key > *current => {
                best_key = Some(key);
                best_entries.clear();
                best_entries.push(entry);
            }
            Some(current) if key == *current => {
                best_entries.push(entry);
            }
            None => {
                best_key = Some(key);
                best_entries.push(entry);
            }
            Some(_) => {}
        }
    }
    pick_preferred_entry(best_entries, mc)
        .with_context(|| format!("no Forge versions available for Minecraft {mc}"))
}

fn pick_preferred_entry(
    matches: Vec<ForgeVersionEntry>,
    mc: &str,
) -> Option<ForgeVersionEntry> {
    if matches.len() <= 1 {
        return matches.into_iter().next();
    }
    if let Some(pos) = matches
        .iter()
        .position(|entry| entry.maven_coord == format!("{mc}-{}", entry.loader_version))
    {
        return matches.into_iter().nth(pos);
    }
    if let Some(pos) = matches
        .iter()
        .position(|entry| entry.maven_coord == format!("{mc}-{}-{mc}", entry.loader_version))
    {
        return matches.into_iter().nth(pos);
    }
    matches.into_iter().next()
}

fn installer_jar_url(maven_coord: &str) -> Result<Url> {
    append_segments(
        FORGE_MAVEN_ROOT,
        &[maven_coord, &format!("forge-{maven_coord}-installer.jar")],
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn entry(mc: &str, loader: &str, coord: &str) -> ForgeVersionEntry {
        ForgeVersionEntry {
            mc_version: mc.to_owned(),
            loader_version: loader.to_owned(),
            maven_coord: coord.to_owned(),
        }
    }

    #[test]
    fn requested_accepts_canonical_and_full_coord_and_rejects_mc_mismatch() {
        let entries = vec![
            entry("1.7.10", "10.13.4.1614", "1.7.10-10.13.4.1614-1.7.10ls"),
            entry("1.7.10", "10.13.4.1614", "1.7.10-10.13.4.1614-1.7.10"),
            entry("1.20.1", "47.2.0", "1.20.1-47.2.0"),
            entry("1.20.4", "49.2.0", "1.20.4-49.2.0"),
        ];

        let modern_short = select_requested(entries.clone(), "1.20.4", "49.2.0").unwrap();
        assert_eq!(modern_short.loader_version, "49.2.0");
        assert_eq!(modern_short.maven_coord, "1.20.4-49.2.0");

        let modern_full = select_requested(entries.clone(), "1.20.4", "1.20.4-49.2.0").unwrap();
        assert_eq!(modern_full.loader_version, "49.2.0");
        assert_eq!(modern_full.maven_coord, "1.20.4-49.2.0");

        let legacy_short = select_requested(entries.clone(), "1.7.10", "10.13.4.1614").unwrap();
        assert_eq!(legacy_short.loader_version, "10.13.4.1614");
        assert_eq!(legacy_short.maven_coord, "1.7.10-10.13.4.1614-1.7.10");

        let legacy_full =
            select_requested(entries.clone(), "1.7.10", "1.7.10-10.13.4.1614-1.7.10").unwrap();
        assert_eq!(legacy_full.loader_version, "10.13.4.1614");

        let legacy_branch =
            select_requested(entries.clone(), "1.7.10", "1.7.10-10.13.4.1614-1.7.10ls").unwrap();
        assert_eq!(legacy_branch.maven_coord, "1.7.10-10.13.4.1614-1.7.10ls");

        // Full Maven coordinate belonging to a different MC version must be rejected.
        assert!(select_requested(entries.clone(), "1.20.4", "1.20.1-47.2.0").is_err());
        assert!(select_requested(entries, "1.20.4", "47.2.0").is_err());
    }

    #[test]
    fn promotions_prefer_recommended_over_latest_and_fallback_picks_highest_numeric() {
        let promos = ForgePromotions {
            recommended: HashMap::from([
                ("1.20.4".into(), "49.2.0".into()),
                ("1.21".into(), "  ".into()),
            ]),
            latest: HashMap::from([
                ("1.20.4".into(), "49.2.1".into()),
                ("1.21".into(), "51.0.33".into()),
            ]),
        };

        assert_eq!(select_promoted(&promos, "1.20.4"), Some("49.2.0"));
        assert_eq!(select_promoted(&promos, "1.21"), Some("51.0.33"));
        assert_eq!(select_promoted(&promos, "1.21.4"), None);

        let entries = vec![
            entry("1.21.4", "54.0.2", "1.21.4-54.0.2"),
            entry("1.21.4", "54.0.10", "1.21.4-54.0.10"),
            entry("1.21.4", "54.0.9", "1.21.4-54.0.9"),
            entry("1.20.4", "99.0.0", "1.20.4-99.0.0"),
        ];
        let latest = select_latest(entries, "1.21.4").unwrap();
        assert_eq!(latest.loader_version, "54.0.10");
        assert_eq!(latest.maven_coord, "1.21.4-54.0.10");

        assert!(select_latest(vec![], "1.21.4").is_err());
    }

    #[test]
    fn official_maven_installer_url_uses_exact_maven_coord() {
        assert_eq!(
            installer_jar_url("1.20.4-49.2.0").unwrap().as_str(),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.4-49.2.0/forge-1.20.4-49.2.0-installer.jar"
        );
        assert_eq!(
            installer_jar_url("1.7.10-10.13.4.1614-1.7.10")
                .unwrap()
                .as_str(),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10-installer.jar"
        );
        assert!(installer_jar_url("../escape").is_err());
    }
}
