//! Sole authlib-injector download API: catalog queries, validation and fixed releases.
//!
//! Raw protocol fields stay private in catalog implementations. A resolved release
//! preserves the same build and SHA-256 across download-source fallback; downloading
//! does not query latest again.

use std::path::Path;

use anyhow::{Context, Result, ensure};

use super::{
    Catalog, Checksum, ChecksumAlgorithm, DownloadOutcome, DownloadRequest, Downloader, Event,
    catalog::AuthlibBuildInfo,
    sources::OfficialCatalog,
};

/// Resolves the latest build reported by the first successful configured source.
/// A mirror may lag behind upstream; this does not merge version catalogs.
pub async fn latest(downloader: &Downloader) -> Result<Release> {
    latest_with_progress(downloader, &|_| Ok(())).await
}

/// Resolves latest through the configured catalog and reports request source changes.
pub async fn latest_with_progress(
    downloader: &Downloader,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<Release> {
    let official = OfficialCatalog
        .authlib_latest()?
        .expect("OfficialCatalog always provides authlib_latest");
    downloader
        .query_catalog_with_progress(
            official,
            |catalog| catalog.authlib_latest(),
            |info| Release::from_catalog(info, downloader),
            progress,
        )
        .await
}

/// Resolves one exact build. A response for a different build is rejected within
/// the request attempt so another configured source can be tried.
pub async fn build(downloader: &Downloader, build_number: u64) -> Result<Release> {
    let official = OfficialCatalog
        .authlib_build(build_number)?
        .expect("OfficialCatalog always provides authlib_build");
    downloader
        .query_catalog(
            official,
            |catalog| catalog.authlib_build(build_number),
            |info| {
                let release = Release::from_catalog(info, downloader)?;
                ensure!(
                    release.build_number == build_number,
                    "authlib-injector returned build {} instead of {build_number}",
                    release.build_number
                );
                Ok(release)
            },
        )
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

    fn from_catalog(info: AuthlibBuildInfo, downloader: &Downloader) -> Result<Self> {
        ensure!(
            !info.version.trim().is_empty(),
            "authlib-injector version is empty"
        );
        let checksum = Checksum::new(ChecksumAlgorithm::Sha256, &info.sha256)
            .context("invalid authlib-injector SHA-256")?;
        let url = downloader.original_url(&info.download_url)?;
        Ok(Self {
            build_number: info.build_number,
            version: info.version,
            release_time: info.release_time,
            request: DownloadRequest::new(url, None, vec![checksum])?,
        })
    }
}
