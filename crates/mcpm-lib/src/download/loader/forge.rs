//! Forge installer version resolution, dual-format matching, and official Maven URL construction.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use url::Url;

use super::{InstallerRelease, LoaderFamily, fetch_sha1, parse_numeric_segments};
use crate::download::{
    DownloadRequest, Downloader, Event, append_path_segments,
    catalog::{ForgePromotions, ForgeVersionEntry},
    sources::{FORGE_MAVEN_ROOT, OfficialCatalog},
};

struct ResolvedForgeEntry {
    loader_version: String,
    maven_coord: String,
}

pub(super) async fn resolve(
    downloader: &Downloader,
    mc: &str,
    requested: Option<&str>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<InstallerRelease> {
    let promoted = match requested {
        Some(_) => None,
        None => {
            let promos_official = OfficialCatalog::forge_promotions_endpoint()?;
            let observer_failed = AtomicBool::new(false);
            let promo_progress = |event: Event<'_>| {
                let result = progress(event);
                if result.is_err() {
                    observer_failed.store(true, Ordering::Relaxed);
                }
                result
            };
            match downloader
                .query_catalog_with_progress(
                    promos_official,
                    |catalog| catalog.forge_promotions(),
                    |promos| Ok(select_promoted(&promos, mc).map(str::to_owned)),
                    &promo_progress,
                )
                .await
            {
                Ok(version) => version,
                Err(err) if observer_failed.load(Ordering::Relaxed) => return Err(err),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        mc,
                        "failed to query Forge promotions; falling back to Forge version list"
                    );
                    None
                }
            }
        }
    };

    let versions_official = OfficialCatalog::forge_versions_endpoint()?;
    let (entry, jar_url) = downloader
        .query_catalog_with_progress(
            versions_official,
            |catalog| catalog.forge_versions(mc),
            |entries| {
                let entry = match (requested, promoted.as_deref()) {
                    (Some(version), _) => select_requested(&entries, mc, version)?,
                    (None, Some(version)) => match select_requested(&entries, mc, version) {
                        Ok(entry) => entry,
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                mc,
                                promoted = version,
                                "promoted Forge version not found in version list; falling back to latest"
                            );
                            select_latest(&entries, mc)?
                        }
                    },
                    (None, None) => select_latest(&entries, mc)?,
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

fn resolve_for_mc(entry: &ForgeVersionEntry, mc: &str) -> Option<ResolvedForgeEntry> {
    match (entry.mc_version.as_deref(), entry.loader_version.as_deref()) {
        (Some(entry_mc), Some(loader_version)) => {
            (entry_mc == mc && !loader_version.is_empty()).then(|| ResolvedForgeEntry {
                loader_version: loader_version.to_owned(),
                maven_coord: entry.maven_coord.clone(),
            })
        }
        (Some(entry_mc), None) if entry_mc != mc => None,
        (_, _) => {
            let rest = entry
                .maven_coord
                .strip_prefix(mc)
                .and_then(|tail| tail.strip_prefix('-'))?;
            let loader_version = rest.split_once('-').map_or(rest, |(version, _)| version);
            (!loader_version.is_empty()).then(|| ResolvedForgeEntry {
                loader_version: loader_version.to_owned(),
                maven_coord: entry.maven_coord.clone(),
            })
        }
    }
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
    entries: &[ForgeVersionEntry],
    mc: &str,
    input: &str,
) -> Result<ResolvedForgeEntry> {
    let matches: Vec<_> = entries
        .iter()
        .filter_map(|entry| resolve_for_mc(entry, mc))
        .filter(|entry| entry.loader_version == input || entry.maven_coord == input)
        .collect();
    pick_preferred_entry(matches, mc)
        .with_context(|| format!("Forge version {input:?} is not available for Minecraft {mc}"))
}

fn select_latest(entries: &[ForgeVersionEntry], mc: &str) -> Result<ResolvedForgeEntry> {
    let mut best_key: Option<Vec<u64>> = None;
    let mut best_numeric = Vec::new();
    let mut non_numeric = Vec::new();
    for entry in entries.iter().filter_map(|entry| resolve_for_mc(entry, mc)) {
        let Some(key) = parse_numeric_segments(&entry.loader_version) else {
            non_numeric.push(entry);
            continue;
        };
        match &best_key {
            Some(current) if key > *current => {
                best_key = Some(key);
                best_numeric.clear();
                best_numeric.push(entry);
            }
            Some(current) if key == *current => {
                best_numeric.push(entry);
            }
            None => {
                best_key = Some(key);
                best_numeric.push(entry);
            }
            Some(_) => {}
        }
    }
    if !best_numeric.is_empty() {
        return pick_preferred_entry(best_numeric, mc)
            .with_context(|| format!("no Forge versions available for Minecraft {mc}"));
    }
    if let Some(last_version) = non_numeric.last().map(|entry| entry.loader_version.clone()) {
        let group: Vec<_> = non_numeric
            .into_iter()
            .filter(|entry| entry.loader_version == last_version)
            .collect();
        return pick_preferred_entry(group, mc)
            .with_context(|| format!("no Forge versions available for Minecraft {mc}"));
    }
    Err(anyhow::anyhow!(
        "no Forge versions available for Minecraft {mc}"
    ))
}

fn pick_preferred_entry(
    matches: Vec<ResolvedForgeEntry>,
    mc: &str,
) -> Option<ResolvedForgeEntry> {
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
    append_path_segments(
        FORGE_MAVEN_ROOT,
        &[maven_coord, &format!("forge-{maven_coord}-installer.jar")],
        "Forge Maven coordinate",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn structured(mc: &str, loader: &str, coord: &str) -> ForgeVersionEntry {
        ForgeVersionEntry {
            maven_coord: coord.to_owned(),
            mc_version: Some(mc.to_owned()),
            loader_version: Some(loader.to_owned()),
        }
    }

    fn raw(coord: &str) -> ForgeVersionEntry {
        ForgeVersionEntry {
            maven_coord: coord.to_owned(),
            mc_version: None,
            loader_version: None,
        }
    }

    #[test]
    fn requested_accepts_canonical_full_coord_arbitrary_and_hyphenated_mc() {
        let entries = vec![
            structured("1.7.10", "10.13.4.1614", "1.7.10-10.13.4.1614-1.7.10ls"),
            structured("1.7.10", "10.13.4.1614", "1.7.10-10.13.4.1614-1.7.10"),
            raw("1.20.1-47.2.0"),
            raw("1.20.4-49.2.0"),
            raw("1.20.4-asdfjk"),
            raw("1.16.5-rc1-36.2.0"),
        ];

        let modern_short = select_requested(&entries, "1.20.4", "49.2.0").unwrap();
        assert_eq!(modern_short.loader_version, "49.2.0");
        assert_eq!(modern_short.maven_coord, "1.20.4-49.2.0");

        let modern_full = select_requested(&entries, "1.20.4", "1.20.4-49.2.0").unwrap();
        assert_eq!(modern_full.loader_version, "49.2.0");
        assert_eq!(modern_full.maven_coord, "1.20.4-49.2.0");

        let arbitrary = select_requested(&entries, "1.20.4", "asdfjk").unwrap();
        assert_eq!(arbitrary.loader_version, "asdfjk");
        assert_eq!(arbitrary.maven_coord, "1.20.4-asdfjk");

        let hyphenated_mc = select_requested(&entries, "1.16.5-rc1", "36.2.0").unwrap();
        assert_eq!(hyphenated_mc.loader_version, "36.2.0");
        assert_eq!(hyphenated_mc.maven_coord, "1.16.5-rc1-36.2.0");

        let legacy_short = select_requested(&entries, "1.7.10", "10.13.4.1614").unwrap();
        assert_eq!(legacy_short.loader_version, "10.13.4.1614");
        assert_eq!(legacy_short.maven_coord, "1.7.10-10.13.4.1614-1.7.10");

        let legacy_full =
            select_requested(&entries, "1.7.10", "1.7.10-10.13.4.1614-1.7.10").unwrap();
        assert_eq!(legacy_full.loader_version, "10.13.4.1614");

        let legacy_branch =
            select_requested(&entries, "1.7.10", "1.7.10-10.13.4.1614-1.7.10ls").unwrap();
        assert_eq!(legacy_branch.maven_coord, "1.7.10-10.13.4.1614-1.7.10ls");

        // Full Maven coordinate belonging to a different MC version must be rejected.
        assert!(select_requested(&entries, "1.20.4", "1.20.1-47.2.0").is_err());
        assert!(select_requested(&entries, "1.20.4", "47.2.0").is_err());
    }

    #[test]
    fn promotions_prefer_recommended_and_latest_tolerates_non_numeric_versions() {
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
            raw("1.21.4-54.0.2"),
            raw("1.21.4-54.0.10"),
            raw("1.21.4-54.0.9"),
            raw("1.21.4-asdfjk"),
            raw("1.20.4-99.0.0"),
        ];
        let latest = select_latest(&entries, "1.21.4").unwrap();
        assert_eq!(latest.loader_version, "54.0.10");
        assert_eq!(latest.maven_coord, "1.21.4-54.0.10");

        let non_numeric_only = vec![raw("1.21.4-first"), raw("1.21.4-asdfjk")];
        let fallback = select_latest(&non_numeric_only, "1.21.4").unwrap();
        assert_eq!(fallback.loader_version, "asdfjk");
        assert_eq!(fallback.maven_coord, "1.21.4-asdfjk");

        assert!(select_latest(&[], "1.21.4").is_err());
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

    #[test]
    fn promotions_observer_failure_aborts_without_falling_back_to_version_list() {
        use std::sync::{Arc, atomic::AtomicUsize};

        use crate::download::{DownloadOptions, DownloadSource, sources::OfficialSource};

        #[derive(Debug, thiserror::Error)]
        #[error("observer aborted promotions")]
        struct ObserverAborted;

        let downloader = Downloader::new(
            vec![Arc::new(OfficialSource) as Arc<dyn DownloadSource>],
            DownloadOptions::default(),
        )
        .unwrap();
        let started = AtomicUsize::new(0);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = runtime
            .block_on(resolve(&downloader, "1.20.4", None, &|event| {
                if matches!(event, Event::AttemptStarted { .. }) {
                    started.fetch_add(1, Ordering::Relaxed);
                    return Err(ObserverAborted.into());
                }
                Ok(())
            }))
            .unwrap_err();
        assert!(crate::error::contains::<ObserverAborted>(error.as_ref()));
        assert_eq!(started.load(Ordering::Relaxed), 1);
    }
}
