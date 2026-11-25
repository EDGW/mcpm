//! Mod loader installer resolution and SHA-1-verified downloads.
//!
//! Resolves one compatible Forge, NeoForge, or Fabric installer for a target
//! Minecraft version, fixes its canonical loader version, official Maven URL,
//! and `.sha1` digest, and downloads through [`Downloader`] without reselecting
//! across source fallbacks.

use std::{fmt, path::Path};

use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{Checksum, ChecksumAlgorithm, DownloadOutcome, DownloadRequest, Downloader, Event};

mod fabric;
mod forge;
mod neoforge;

/// Supported mod loader families whose installers can be resolved and downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderFamily {
    Forge,
    NeoForge,
    Fabric,
}

impl LoaderFamily {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
            Self::Fabric => "fabric",
        }
    }
}

impl fmt::Display for LoaderFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Fixed, SHA-1-verified loader installer artifact.
///
/// Once resolved, downloading across mirror fallbacks always fetches the same
/// official artifact URL and verifies the same SHA-1 digest without querying
/// version catalogs again.
#[derive(Debug, Clone)]
pub struct InstallerRelease {
    family: LoaderFamily,
    loader_version: String,
    request: DownloadRequest,
}

impl InstallerRelease {
    pub(super) fn new(
        family: LoaderFamily,
        loader_version: String,
        request: DownloadRequest,
    ) -> Self {
        Self {
            family,
            loader_version,
            request,
        }
    }

    pub fn family(&self) -> LoaderFamily {
        self.family
    }

    pub fn loader_version(&self) -> &str {
        &self.loader_version
    }

    /// Downloads and verifies the installer JAR to an absent destination.
    pub async fn download_to(
        &self,
        downloader: &Downloader,
        destination: &Path,
    ) -> Result<DownloadOutcome> {
        self.download_to_with_progress(downloader, destination, &|_| Ok(()))
            .await
    }

    /// Downloads the fixed installer JAR while reporting transfer progress.
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
                    "failed to download {} {} installer",
                    self.family, self.loader_version
                )
            })
    }
}

/// Resolves a compatible loader installer for `minecraft_version` without progress events.
pub async fn resolve(
    downloader: &Downloader,
    family: LoaderFamily,
    minecraft_version: &str,
    loader_version: Option<&str>,
) -> Result<InstallerRelease> {
    resolve_with_progress(
        downloader,
        family,
        minecraft_version,
        loader_version,
        &|_| Ok(()),
    )
    .await
}

/// Resolves a compatible loader installer for `minecraft_version` while reporting
/// catalog and `.sha1` metadata progress events.
pub async fn resolve_with_progress(
    downloader: &Downloader,
    family: LoaderFamily,
    minecraft_version: &str,
    loader_version: Option<&str>,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<InstallerRelease> {
    let minecraft_version = minecraft_version.trim();
    ensure!(!minecraft_version.is_empty(), "Minecraft version is empty");
    let loader_version = match loader_version {
        Some(version) => {
            let trimmed = version.trim();
            ensure!(!trimmed.is_empty(), "{family} loader version is empty");
            Some(trimmed)
        }
        None => None,
    };

    match family {
        LoaderFamily::Forge => {
            forge::resolve(downloader, minecraft_version, loader_version, progress).await
        }
        LoaderFamily::NeoForge => {
            neoforge::resolve(downloader, minecraft_version, loader_version, progress).await
        }
        LoaderFamily::Fabric => {
            fabric::resolve(downloader, minecraft_version, loader_version, progress).await
        }
    }
    .with_context(|| match loader_version {
        Some(version) => format!(
            "failed to resolve {family} {version} installer for Minecraft {minecraft_version}"
        ),
        None => format!("failed to resolve {family} installer for Minecraft {minecraft_version}"),
    })
}

pub(super) async fn fetch_sha1(
    downloader: &Downloader,
    jar_url: &Url,
    progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
) -> Result<Checksum> {
    let sha1_url = Url::parse(&format!("{jar_url}.sha1"))
        .with_context(|| format!("failed to construct SHA-1 URL for {jar_url}"))?;
    downloader
        .metadata_with_progress(&sha1_url, parse_sha1_text, progress)
        .await
        .with_context(|| format!("failed to fetch SHA-1 checksum for {jar_url}"))
}

pub(super) fn parse_sha1_text(bytes: &[u8]) -> Result<Checksum> {
    let text = std::str::from_utf8(bytes).context("SHA-1 checksum file is not valid UTF-8")?;
    let token = text
        .split_whitespace()
        .next()
        .context("SHA-1 checksum file is empty")?;
    Checksum::new(ChecksumAlgorithm::Sha1, token).context("invalid SHA-1 checksum file")
}

pub(super) fn append_segments(base: &str, segments: &[&str]) -> Result<Url> {
    let mut url = Url::parse(base).with_context(|| format!("invalid base URL {base:?}"))?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| anyhow!("cannot append path segments to {base}"))?;
        path.pop_if_empty();
        for segment in segments {
            ensure!(
                is_valid_segment(segment),
                "invalid URL path segment: {segment:?}"
            );
            path.push(segment);
        }
    }
    Ok(url)
}

pub(super) fn parse_numeric_segments(version: &str) -> Option<Vec<u64>> {
    if version.is_empty() {
        return None;
    }
    version
        .split('.')
        .map(|segment| {
            if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            segment.parse::<u64>().ok()
        })
        .collect()
}

fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '/' | '\\' | '?' | '#'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sha1_text_accepts_plain_annotated_and_crlf_and_rejects_invalid() {
        let hex = "0123456789ABCDEF0123456789abcdef01234567";
        let lower = "0123456789abcdef0123456789abcdef01234567";

        assert_eq!(parse_sha1_text(hex.as_bytes()).unwrap().value(), lower);
        assert_eq!(
            parse_sha1_text(format!("  {hex}  forge-installer.jar\r\n").as_bytes())
                .unwrap()
                .value(),
            lower
        );
        assert_eq!(
            parse_sha1_text(format!("\r\n{lower}\n").as_bytes())
                .unwrap()
                .value(),
            lower
        );

        assert!(parse_sha1_text(b"").is_err());
        assert!(parse_sha1_text(b"   \r\n").is_err());
        assert!(parse_sha1_text(b"0123456789abcdef").is_err());
        assert!(parse_sha1_text(b"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
        assert!(parse_sha1_text(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn append_segments_rejects_traversal_and_special_characters() {
        let url = append_segments("https://maven.example.invalid/base", &["1.0", "a.jar"]).unwrap();
        assert_eq!(url.as_str(), "https://maven.example.invalid/base/1.0/a.jar");

        for bad in ["", ".", "..", "a/b", "a\\b", "a?b", "a#b", "a b", "a\x00b"] {
            assert!(
                append_segments("https://maven.example.invalid/", &[bad]).is_err(),
                "expected segment {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn quiet_loader_apis_are_send() {
        fn assert_send(_: impl Send) {}
        fn check(downloader: &Downloader, release: &InstallerRelease, destination: &Path) {
            assert_send(resolve(downloader, LoaderFamily::Forge, "1.20.4", None));
            assert_send(release.download_to(downloader, destination));
        }
        let _ = check;
    }
}
