//! Sole authlib-injector download API: endpoints, upstream decoding and fixed releases.
//!
//! Raw protocol fields stay private. A resolved release preserves the same build
//! and SHA-256 across download-source fallback; downloading does not query latest again.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use url::Url;

use super::{Checksum, ChecksumAlgorithm, DownloadOutcome, DownloadRequest, Downloader, Event};

const API_ROOT: &str = "https://authlib-injector.yushi.moe/";

/// Resolves the latest build reported by the first successful configured source.
/// A mirror may lag behind upstream; this does not merge version catalogs.
pub async fn latest(downloader: &Downloader) -> Result<Release> {
    latest_with_progress(downloader, &|_| Ok(())).await
}

/// Resolves latest through the sole upstream decoder and reports request source changes.
pub async fn latest_with_progress(
    downloader: &Downloader,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<Release> {
    let url = endpoint("artifact/latest.json")?;
    downloader
        .metadata_with_progress(&url, |bytes| Release::decode(bytes, downloader), progress)
        .await
}

/// Resolves one exact build. A response for a different build is rejected within
/// the request attempt so another configured source can be tried.
pub async fn build(downloader: &Downloader, build_number: u64) -> Result<Release> {
    let url = endpoint(&format!("artifact/{build_number}.json"))?;
    downloader
        .metadata(&url, |bytes| {
            let release = Release::decode(bytes, downloader)?;
            ensure!(
                release.build_number == build_number,
                "authlib-injector returned build {} instead of {build_number}",
                release.build_number
            );
            Ok(release)
        })
        .await
}

/// A fixed upstream build with validated download expectations.
/// Its private protocol representation cannot be accessed or independently decoded.
#[derive(Debug, Clone)]
pub struct Release {
    build_number: u64,
    version: String,
    release_time: Option<String>,
    request: DownloadRequest,
}

impl Release {
    pub fn build_number(&self) -> u64 {
        self.build_number
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn release_time(&self) -> Option<&str> {
        self.release_time.as_deref()
    }

    pub fn sha256(&self) -> &str {
        self.request.checksums()[0].value()
    }

    /// Downloads and verifies this build to an absent destination. No Java
    /// process is executed and no server configuration is changed.
    pub async fn download_to(
        &self,
        downloader: &Downloader,
        destination: &Path,
    ) -> Result<DownloadOutcome> {
        self.download_to_with_progress(downloader, destination, &|_| Ok(()))
            .await
    }

    /// Reports file transfer progress without reselecting the fixed release on fallback.
    pub async fn download_to_with_progress(
        &self,
        downloader: &Downloader,
        destination: &Path,
        progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
    ) -> Result<DownloadOutcome> {
        downloader
            .download_to_with_progress(&self.request, destination, progress)
            .await
            .with_context(|| {
                format!(
                    "failed to download authlib-injector build {}",
                    self.build_number
                )
            })
    }

    fn decode(bytes: &[u8], downloader: &Downloader) -> Result<Self> {
        let metadata: Metadata =
            serde_json::from_slice(bytes).context("invalid authlib-injector release JSON")?;
        ensure!(
            !metadata.version.trim().is_empty(),
            "authlib-injector version is empty"
        );
        let checksum = Checksum::new(ChecksumAlgorithm::Sha256, &metadata.checksums.sha256)
            .context("invalid authlib-injector SHA-256")?;
        let url = downloader.original_url(&metadata.download_url)?;
        Ok(Self {
            build_number: metadata.build_number,
            version: metadata.version,
            release_time: metadata.release_time,
            request: DownloadRequest::new(url, None, vec![checksum])?,
        })
    }
}

fn endpoint(path: &str) -> Result<Url> {
    Ok(Url::parse(API_ROOT)?.join(path)?)
}

#[derive(Deserialize)]
struct Metadata {
    build_number: u64,
    version: String,
    #[serde(default)]
    release_time: Option<String>,
    download_url: Url,
    checksums: Checksums,
}

#[derive(Deserialize)]
struct Checksums {
    sha256: String,
}
