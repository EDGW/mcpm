//! Mojang's version catalog and fixed, verified vanilla server downloads.

use std::{io::Cursor, path::Path};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{
    Catalog, Checksum, ChecksumAlgorithm, DownloadOutcome, DownloadRequest, Downloader, Event,
    catalog::{MinecraftVersionEntry, MinecraftVersionList},
    sources::OfficialCatalog,
};
use crate::local::codec::Codec;

/// Resolves an exact version, latest release, or latest snapshot through configured sources.
/// Selection and validation happen inside each attempt so a lagging mirror can fall back.
pub async fn resolve_server(
    downloader: &Downloader,
    version: &str,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<ServerRelease> {
    ensure!(!version.trim().is_empty(), "Minecraft version is empty");
    let official = OfficialCatalog
        .minecraft_versions()?
        .expect("OfficialCatalog always provides minecraft_versions");
    let selected = downloader
        .query_catalog_with_progress(
            official,
            |catalog| catalog.minecraft_versions(),
            |list| select(list, version),
            progress,
        )
        .await?;
    let url = downloader.original_url(&selected.url)?;
    downloader
        .metadata_with_progress(
            &url,
            |bytes| {
                if let Some(digest) = &selected.sha1 {
                    Checksum::new(ChecksumAlgorithm::Sha1, digest)?
                        .verify_reader(&mut Cursor::new(bytes))?;
                }
                decode_release(bytes, &selected.id, selected.release_time, downloader)
            },
            progress,
        )
        .await
        .with_context(|| format!("failed to resolve Minecraft {} server", selected.id))
}

/// Immutable server artifact; source fallback never reselects its version or digest.
#[derive(Debug)]
pub struct ServerRelease {
    id: String,
    request: DownloadRequest,
    java_major: Option<u32>,
    release_time: DateTime<FixedOffset>,
}

impl ServerRelease {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn java_major(&self) -> Option<u32> {
        self.java_major
    }

    /// Original publication time from Mojang's catalog, not metadata modification time.
    pub fn release_time(&self) -> DateTime<FixedOffset> {
        self.release_time
    }

    pub async fn download_to_with_progress(
        &self,
        downloader: &Downloader,
        destination: &Path,
        progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
    ) -> Result<DownloadOutcome> {
        downloader
            .download_to_with_progress(&self.request, destination, progress)
            .await
            .with_context(|| format!("failed to download Minecraft {} server", self.id))
    }
}

/// Sole transport codec for an already resolved artifact, preserving the exact checksum.
impl Codec for ServerRelease {
    fn encode(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&ResolvedRelease {
            id: self.id.clone(),
            url: self.request.url().clone(),
            size: self.request.size(),
            sha1: self.request.checksums()[0].value().into(),
            java_major: self.java_major,
            release_time: self.release_time,
        })?)
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let resolved: ResolvedRelease =
            serde_json::from_slice(bytes).context("invalid resolved Minecraft server artifact")?;
        ensure!(!resolved.id.trim().is_empty(), "Minecraft version is empty");
        Ok(Self {
            id: resolved.id,
            request: DownloadRequest::new(
                resolved.url,
                resolved.size,
                vec![Checksum::new(ChecksumAlgorithm::Sha1, &resolved.sha1)?],
            )?,
            java_major: resolved.java_major,
            release_time: resolved.release_time,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolvedRelease {
    id: String,
    url: Url,
    size: Option<u64>,
    sha1: String,
    java_major: Option<u32>,
    release_time: DateTime<FixedOffset>,
}

pub(super) fn select(
    catalog: MinecraftVersionList,
    requested: &str,
) -> Result<MinecraftVersionEntry> {
    let id = match requested {
        "latest" => {
            ensure!(
                !catalog.latest_release.trim().is_empty(),
                "latest release is missing from Mojang catalog"
            );
            catalog.latest_release.as_str()
        }
        "latest-snapshot" => {
            ensure!(
                !catalog.latest_snapshot.trim().is_empty(),
                "latest snapshot is missing from Mojang catalog"
            );
            catalog.latest_snapshot.as_str()
        }
        id => id,
    };
    let entry = catalog
        .entries
        .into_iter()
        .find(|entry| entry.id == id)
        .with_context(|| format!("unknown Minecraft version {id:?}"))?;
    DownloadRequest::new(entry.url.clone(), None, vec![])?;
    if let Some(digest) = &entry.sha1 {
        Checksum::new(ChecksumAlgorithm::Sha1, digest)?;
    }
    Ok(entry)
}

fn decode_release(
    bytes: &[u8],
    expected: &str,
    release_time: DateTime<FixedOffset>,
    downloader: &Downloader,
) -> Result<ServerRelease> {
    let document: Version =
        serde_json::from_slice(bytes).context("invalid Minecraft version JSON")?;
    ensure!(
        document.id == expected,
        "Minecraft version JSON returned {:?} instead of {expected:?}",
        document.id
    );
    let server = document
        .downloads
        .and_then(|downloads| downloads.server)
        .with_context(|| format!("Minecraft {expected} has no dedicated server download"))?;
    let digest = Checksum::new(ChecksumAlgorithm::Sha1, &server.sha1)?;
    let request = DownloadRequest::new(
        downloader.original_url(&server.url)?,
        server.size,
        vec![digest],
    )?;
    Ok(ServerRelease {
        id: document.id,
        request,
        java_major: document.java_version.map(|java| java.major_version),
        release_time,
    })
}

#[derive(Deserialize)]
struct Version {
    id: String,
    downloads: Option<Downloads>,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersion>,
}
#[derive(Deserialize)]
struct Downloads {
    server: Option<Artifact>,
}
#[derive(Deserialize)]
struct Artifact {
    url: Url,
    sha1: String,
    size: Option<u64>,
}
#[derive(Deserialize)]
struct JavaVersion {
    #[serde(rename = "majorVersion")]
    major_version: u32,
}

