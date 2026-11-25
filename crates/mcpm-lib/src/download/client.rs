//! Shared ordered requests, private metadata delivery and verified file publication.

use std::{
    collections::HashSet,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use bytes::Bytes;
use reqwest::{Client, Response};
use tokio::io::AsyncWriteExt;
use url::Url;

use super::{
    Catalog, CatalogEndpoint, DownloadRequest, DownloadSource, Event, RequestKind,
    integrity::{Verifier, validate_url},
};
use mcpm_exec::error::CombinedError;

/// HTTP downloader with explicit source order. No source is added implicitly.
/// Dropping a request future cancels it; unfinished temporary files are discarded.
pub struct Downloader {
    client: Client,
    sources: Vec<Arc<dyn DownloadSource>>,
}

impl Downloader {
    pub fn new(sources: Vec<Arc<dyn DownloadSource>>, options: DownloadOptions) -> Result<Self> {
        options.validate()?;
        let mut ids = HashSet::new();
        for source in &sources {
            ensure!(
                !source.id().trim().is_empty(),
                "download source ID cannot be empty"
            );
            ensure!(
                ids.insert(source.id()),
                "duplicate download source {:?}",
                source.id()
            );
        }
        let client = Client::builder()
            .connect_timeout(options.connect_timeout)
            .read_timeout(options.read_timeout)
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .context("failed to build download HTTP client")?;
        Ok(Self { client, sources })
    }

    /// Configured download sources in attempt order.
    pub fn sources(&self) -> &[Arc<dyn DownloadSource>] {
        &self.sources
    }

    /// Restores mirror addresses using configured mapping rules. Ambiguous rules
    /// across sources are errors; unknown addresses are preserved without guessing.
    pub fn original_url(&self, url: &Url) -> Result<Url> {
        validate_url(url)?;
        let mut original = None;
        for source in &self.sources {
            if let Some(restored) = source.original_url(url).with_context(|| {
                format!(
                    "failed to restore URL using download source {:?}",
                    source.id()
                )
            })? {
                validate_url(&restored)?;
                if let Some(existing) = &original {
                    ensure!(existing == &restored, "conflicting original URLs for {url}");
                }
                original = Some(restored);
            }
        }
        Ok(original.unwrap_or_else(|| url.clone()))
    }

    /// Streams to a temporary file next to the destination, verifies all declared
    /// expectations, then publishes without replacing an existing file. Parent
    /// directories must exist. Local I/O failures stop immediately rather than
    /// requesting the file again from another website.
    pub async fn download_to(
        &self,
        request: &DownloadRequest,
        destination: &Path,
    ) -> Result<DownloadOutcome> {
        self.download_to_with_progress(request, destination, &|_| Ok(()))
            .await
    }

    /// Downloads through the same publication path while reporting per-attempt byte counts.
    pub async fn download_to_with_progress(
        &self,
        request: &DownloadRequest,
        destination: &Path,
        progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
    ) -> Result<DownloadOutcome> {
        ensure!(
            destination.file_name().is_some(),
            "download destination must name a file"
        );
        match tokio::fs::symlink_metadata(destination).await {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect download destination {destination:?}")
                });
            }
            Ok(_) => bail!("download destination already exists: {destination:?}"),
        }
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let (staged, size, url) = self
            .try_urls(
                request.url(),
                RequestKind::File,
                progress,
                |url| async move {
                    let temporary = tempfile::NamedTempFile::new_in(parent)
                        .context("failed to create temporary download file")
                        .map_err(AttemptError::Local)?;
                    let (file, staged) = temporary.into_parts();
                    let mut file = tokio::fs::File::from_std(file);
                    let mut response = self.response(url).await.map_err(AttemptError::Remote)?;
                    let url = response.url().clone();
                    let total = request.size().or_else(|| response.content_length());
                    progress(Event::Bytes { received: 0, total }).map_err(AttemptError::Local)?;
                    let mut verifier = Verifier::new(request);
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .context("failed to read download response")
                        .map_err(AttemptError::Remote)?
                    {
                        verifier.update(&chunk).map_err(AttemptError::Remote)?;
                        file.write_all(&chunk)
                            .await
                            .context("failed to write temporary download file")
                            .map_err(AttemptError::Local)?;
                        progress(Event::Bytes {
                            received: verifier.received(),
                            total,
                        })
                        .map_err(AttemptError::Local)?;
                    }
                    progress(Event::Verifying).map_err(AttemptError::Local)?;
                    let size = verifier.finish().map_err(AttemptError::Remote)?;
                    file.flush()
                        .await
                        .context("failed to flush temporary download file")
                        .map_err(AttemptError::Local)?;
                    drop(file);
                    Ok((staged, size, url))
                },
            )
            .await?;
        staged
            .persist_noclobber(destination)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to publish download to {destination:?}"))?;
        progress(Event::Completed {
            kind: RequestKind::File,
        })?;
        Ok(DownloadOutcome {
            destination: destination.to_owned(),
            url,
            size,
        })
    }

    /// Only target wrappers receive response bytes. Decoding and semantic checks
    /// happen within each attempt, so malformed metadata can fall back too.
    pub(super) async fn metadata<T>(
        &self,
        url: &Url,
        decode: impl Fn(&[u8]) -> Result<T>,
    ) -> Result<T> {
        self.metadata_with_progress(url, decode, &|_| Ok(())).await
    }

    pub(super) async fn metadata_with_progress<T>(
        &self,
        url: &Url,
        decode: impl Fn(&[u8]) -> Result<T>,
        progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
    ) -> Result<T> {
        let decode = &decode;
        let value = self
            .try_urls(url, RequestKind::Metadata, progress, |url| async move {
                let bytes = self.fetch_bytes(url).await.map_err(AttemptError::Remote)?;
                decode(&bytes)
                    .context("invalid download metadata")
                    .map_err(AttemptError::Remote)
            })
            .await?;
        progress(Event::Completed {
            kind: RequestKind::Metadata,
        })?;
        Ok(value)
    }

    /// Queries a version catalog across configured sources without emitting progress events.
    pub(super) async fn query_catalog<T, R>(
        &self,
        official: CatalogEndpoint<T>,
        override_for: impl Fn(&dyn Catalog) -> Result<Option<CatalogEndpoint<T>>>,
        select: impl Fn(T) -> Result<R>,
    ) -> Result<R> {
        self.query_catalog_with_progress(official, override_for, select, &|_| Ok(()))
            .await
    }

    /// Queries a version catalog across configured sources, allowing mirror-specific
    /// catalog overrides and per-attempt selection so lagging mirrors can fall back.
    pub(super) async fn query_catalog_with_progress<T, R>(
        &self,
        official: CatalogEndpoint<T>,
        override_for: impl Fn(&dyn Catalog) -> Result<Option<CatalogEndpoint<T>>>,
        select: impl Fn(T) -> Result<R>,
        progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
    ) -> Result<R> {
        validate_url(&official.url)?;
        let mut seen = HashSet::new();
        let mut errors = Vec::new();
        for source in &self.sources {
            let candidates = if let Some(catalog) = source.catalog()
                && let Some(custom) = override_for(catalog).with_context(|| {
                    format!(
                        "failed to resolve catalog endpoint for download source {:?}",
                        source.id()
                    )
                })? {
                vec![custom]
            } else {
                source
                    .translate(&official.url)
                    .with_context(|| {
                        format!(
                            "failed to translate {} using download source {:?}",
                            official.url,
                            source.id()
                        )
                    })?
                    .into_iter()
                    .map(|url| CatalogEndpoint {
                        url,
                        decode: official.decode,
                    })
                    .collect()
            };
            for endpoint in candidates {
                validate_url(&endpoint.url)?;
                if !seen.insert(endpoint.url.clone()) {
                    continue;
                }
                progress(Event::AttemptStarted {
                    kind: RequestKind::Metadata,
                    source: source.id(),
                })?;
                let attempt = match self.fetch_bytes(endpoint.url.clone()).await {
                    Ok(bytes) => (endpoint.decode)(&bytes)
                        .context("invalid download metadata")
                        .and_then(&select),
                    Err(error) => Err(error),
                };
                match attempt {
                    Ok(value) => {
                        progress(Event::Completed {
                            kind: RequestKind::Metadata,
                        })?;
                        return Ok(value);
                    }
                    Err(error) => {
                        let error = error.context(format!(
                            "download source {:?} failed for {}",
                            source.id(),
                            endpoint.url
                        ));
                        if let Err(observer) = progress(Event::AttemptFailed {
                            kind: RequestKind::Metadata,
                            source: source.id(),
                        }) {
                            return Err(CombinedError::new(error, observer).into());
                        }
                        errors.push(error);
                    }
                }
            }
        }
        Err(finish_attempts(&official.url, errors))
    }

    async fn fetch_bytes(&self, url: Url) -> Result<Bytes> {
        self.response(url)
            .await?
            .bytes()
            .await
            .context("failed to read download metadata")
    }

    async fn response(&self, url: Url) -> Result<Response> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("download HTTP request failed")?
            .error_for_status()
            .context("download HTTP status error")?;
        ensure!(
            response.status().is_success(),
            "unexpected download HTTP status {}",
            response.status()
        );
        Ok(response)
    }

    async fn try_urls<T, F, Fut>(
        &self,
        original: &Url,
        kind: RequestKind,
        progress: &(impl Fn(Event<'_>) -> Result<()> + ?Sized),
        mut attempt: F,
    ) -> Result<T>
    where
        F: FnMut(Url) -> Fut,
        Fut: Future<Output = std::result::Result<T, AttemptError>>,
    {
        let original = self.original_url(original)?;
        let mut seen = HashSet::new();
        let mut errors = Vec::new();
        for source in &self.sources {
            let urls = source.translate(&original).with_context(|| {
                format!(
                    "failed to translate {original} using download source {:?}",
                    source.id()
                )
            })?;
            for url in urls {
                validate_url(&url)?;
                if !seen.insert(url.clone()) {
                    continue;
                }
                progress(Event::AttemptStarted {
                    kind,
                    source: source.id(),
                })?;
                match attempt(url.clone()).await {
                    Ok(value) => return Ok(value),
                    Err(AttemptError::Local(error)) => return Err(error),
                    Err(AttemptError::Remote(error)) => {
                        let error = error.context(format!(
                            "download source {:?} failed for {url}",
                            source.id()
                        ));
                        if let Err(observer) = progress(Event::AttemptFailed {
                            kind,
                            source: source.id(),
                        }) {
                            return Err(CombinedError::new(error, observer).into());
                        }
                        errors.push(error);
                    }
                }
            }
        }
        Err(finish_attempts(&original, errors))
    }
}

fn finish_attempts(original: &Url, errors: Vec<anyhow::Error>) -> anyhow::Error {
    let mut errors = errors.into_iter();
    let Some(first) = errors.next() else {
        return anyhow!("no configured download source supports {original}");
    };
    let error = if let Some(second) = errors.next() {
        let mut combined = CombinedError::new(first, second);
        for error in errors {
            combined.push(error);
        }
        anyhow::Error::new(combined)
    } else {
        first
    };
    error.context(format!("failed to download {original}"))
}

/// Network limits shared by metadata and file requests. Timeouts apply to
/// connection establishment and each response read, not the total file duration.
#[derive(Debug, Clone, Copy)]
pub struct DownloadOptions {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
}

impl DownloadOptions {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            !self.connect_timeout.is_zero(),
            "download connect timeout must be positive"
        );
        ensure!(
            !self.read_timeout.is_zero(),
            "download read timeout must be positive"
        );
        Ok(())
    }
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
        }
    }
}

/// A verified and published file; `url` is the final HTTP address after redirects.
#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    pub destination: PathBuf,
    pub url: Url,
    pub size: u64,
}

enum AttemptError {
    Remote(anyhow::Error),
    Local(anyhow::Error),
}
