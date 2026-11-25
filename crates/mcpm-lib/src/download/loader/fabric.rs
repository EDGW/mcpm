//! Fabric loader and installer resolution and official Maven JAR URL construction.

use anyhow::{Context, Result, bail, ensure};
use url::Url;

use super::{InstallerRelease, LoaderFamily, append_segments, fetch_sha1};
use crate::download::{
    Catalog, DownloadRequest, Downloader, Event,
    catalog::{FabricInstallerEntry, FabricLoaderEntry},
    sources::OfficialCatalog,
};

const FABRIC_MAVEN_ROOT: &str = "https://maven.fabricmc.net/";

pub(super) async fn resolve(
    downloader: &Downloader,
    mc: &str,
    requested: Option<&str>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<InstallerRelease> {
    let loaders_official = OfficialCatalog
        .fabric_loaders(mc)?
        .expect("OfficialCatalog always provides fabric_loaders");
    let loader = downloader
        .query_catalog_with_progress(
            loaders_official,
            |catalog| catalog.fabric_loaders(mc),
            |entries| select_loader(entries, mc, requested),
            progress,
        )
        .await?;

    let installers_official = OfficialCatalog
        .fabric_installers()?
        .expect("OfficialCatalog always provides fabric_installers");
    let jar_url = downloader
        .query_catalog_with_progress(
            installers_official,
            |catalog| catalog.fabric_installers(),
            |entries| {
                let installer = select_installer(entries)?;
                installer_jar_url(&installer.maven_coord)
            },
            progress,
        )
        .await?;

    let sha1 = fetch_sha1(downloader, &jar_url, progress).await?;
    let request = DownloadRequest::new(jar_url, None, vec![sha1])?;
    Ok(InstallerRelease::new(
        LoaderFamily::Fabric,
        loader.version,
        request,
    ))
}

fn select_loader(
    entries: Vec<FabricLoaderEntry>,
    mc: &str,
    requested: Option<&str>,
) -> Result<FabricLoaderEntry> {
    if let Some(input) = requested {
        return entries
            .into_iter()
            .find(|entry| entry.version == input)
            .with_context(|| {
                format!("Fabric loader version {input:?} is not available for Minecraft {mc}")
            });
    }

    let mut iter = entries.into_iter();
    let first = iter
        .next()
        .with_context(|| format!("no Fabric loader versions available for Minecraft {mc}"))?;
    if first.stable {
        return Ok(first);
    }
    Ok(iter.find(|entry| entry.stable).unwrap_or(first))
}

fn select_installer(entries: Vec<FabricInstallerEntry>) -> Result<FabricInstallerEntry> {
    let mut iter = entries.into_iter();
    let first = iter
        .next()
        .context("no Fabric installer versions available")?;
    if first.stable {
        return Ok(first);
    }
    Ok(iter.find(|entry| entry.stable).unwrap_or(first))
}

fn installer_jar_url(maven_coord: &str) -> Result<Url> {
    let mut parts = maven_coord.split(':');
    let (Some(group), Some(artifact), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        bail!("invalid Fabric installer Maven coordinate: {maven_coord:?}");
    };
    ensure!(
        !group.is_empty() && !artifact.is_empty() && !version.is_empty(),
        "invalid Fabric installer Maven coordinate: {maven_coord:?}"
    );
    let jar_name = format!("{artifact}-{version}.jar");
    let mut segments: Vec<&str> = group.split('.').collect();
    ensure!(
        !segments.is_empty() && segments.iter().all(|segment| !segment.is_empty()),
        "invalid Fabric installer Maven group: {maven_coord:?}"
    );
    segments.push(artifact);
    segments.push(version);
    segments.push(&jar_name);
    append_segments(FABRIC_MAVEN_ROOT, &segments)
        .with_context(|| format!("invalid Fabric installer Maven coordinate: {maven_coord:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loader(version: &str, stable: bool) -> FabricLoaderEntry {
        FabricLoaderEntry {
            version: version.to_owned(),
            stable,
        }
    }

    fn installer(version: &str, coord: &str, stable: bool) -> FabricInstallerEntry {
        FabricInstallerEntry {
            version: version.to_owned(),
            maven_coord: coord.to_owned(),
            stable,
        }
    }

    #[test]
    fn loader_selection_prefers_first_stable_falls_back_to_first_and_supports_exact_match() {
        let mixed = vec![
            loader("0.16.11-beta.1", false),
            loader("0.16.10", true),
            loader("0.16.9", true),
        ];
        assert_eq!(
            select_loader(mixed.clone(), "1.21.4", None)
                .unwrap()
                .version,
            "0.16.10"
        );
        assert_eq!(
            select_loader(mixed.clone(), "1.21.4", Some("0.16.11-beta.1"))
                .unwrap()
                .version,
            "0.16.11-beta.1"
        );
        assert!(select_loader(mixed, "1.21.4", Some("0.15.0")).is_err());

        let all_unstable = vec![loader("0.17.0-beta.2", false), loader("0.17.0-beta.1", false)];
        assert_eq!(
            select_loader(all_unstable, "1.21.4", None).unwrap().version,
            "0.17.0-beta.2"
        );

        assert!(select_loader(vec![], "1.21.4", None).is_err());
    }

    #[test]
    fn installer_selection_prefers_first_stable_and_falls_back_to_first() {
        let mixed = vec![
            installer("1.1.1-beta", "net.fabricmc:fabric-installer:1.1.1-beta", false),
            installer("1.1.0", "net.fabricmc:fabric-installer:1.1.0", true),
            installer("1.0.1", "net.fabricmc:fabric-installer:1.0.1", true),
        ];
        assert_eq!(select_installer(mixed).unwrap().version, "1.1.0");

        let all_unstable = vec![
            installer("1.2.0-rc.1", "net.fabricmc:fabric-installer:1.2.0-rc.1", false),
            installer("1.2.0-beta.1", "net.fabricmc:fabric-installer:1.2.0-beta.1", false),
        ];
        assert_eq!(select_installer(all_unstable).unwrap().version, "1.2.0-rc.1");

        assert!(select_installer(vec![]).is_err());
    }

    #[test]
    fn maven_coord_builds_official_fabric_jar_url_and_rejects_malformed() {
        assert_eq!(
            installer_jar_url("net.fabricmc:fabric-installer:1.1.0")
                .unwrap()
                .as_str(),
            "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.1.0/fabric-installer-1.1.0.jar"
        );

        for bad in [
            "",
            "net.fabricmc:fabric-installer",
            "net.fabricmc:fabric-installer:1.1.0:extra",
            ":fabric-installer:1.1.0",
            "net..fabricmc:fabric-installer:1.1.0",
            ".net.fabricmc:fabric-installer:1.1.0",
            "net.fabricmc::1.1.0",
            "net.fabricmc:fabric-installer:",
            "net.fabricmc:../escape:1.1.0",
            "net.fabricmc:fabric-installer:..",
            "net.fabricmc:fabric-installer:1.1.0?foo=bar",
        ] {
            assert!(
                installer_jar_url(bad).is_err(),
                "expected Maven coordinate {bad:?} to be rejected"
            );
        }
    }
}
