//! Single TOML codec for download-source order and network timeouts.

use std::{collections::HashSet, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::DownloadOptions;
use crate::local::codec::Codec;

/// Download settings; decoding never accesses the network or constructs sources.
/// Source lists replace defaults completely, including an explicitly empty list.
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    sources: Vec<String>,
    options: DownloadOptions,
}

impl DownloadConfig {
    pub fn new(sources: Vec<String>, options: DownloadOptions) -> Result<Self> {
        let mut seen = HashSet::new();
        for source in &sources {
            ensure!(
                !source.trim().is_empty(),
                "download source ID cannot be empty"
            );
            ensure!(seen.insert(source), "duplicate download source {source:?}");
        }
        options.validate()?;
        Ok(Self { sources, options })
    }

    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    pub fn options(&self) -> DownloadOptions {
        self.options
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            sources: vec!["bmclapi".into(), "official".into()],
            options: DownloadOptions::default(),
        }
    }
}

impl Codec for DownloadConfig {
    fn decode(bytes: &[u8]) -> Result<Self> {
        let document: Document = toml::from_str(
            std::str::from_utf8(bytes).context("download configuration is not UTF-8")?,
        )
        .context("invalid download configuration TOML")?;
        Self::new(
            document.sources,
            DownloadOptions {
                connect_timeout: Duration::from_secs(document.connect_timeout_seconds),
                read_timeout: Duration::from_secs(document.read_timeout_seconds),
            },
        )
    }

    fn encode(&self) -> Result<Vec<u8>> {
        let seconds = |duration: Duration| -> Result<u64> {
            ensure!(
                duration.subsec_nanos() == 0,
                "download configuration timeouts must use whole seconds"
            );
            Ok(duration.as_secs())
        };
        let document = Document {
            sources: self.sources.clone(),
            connect_timeout_seconds: seconds(self.options.connect_timeout)?,
            read_timeout_seconds: seconds(self.options.read_timeout)?,
        };
        Ok(toml::to_string_pretty(&document)?.into_bytes())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Document {
    sources: Vec<String>,
    connect_timeout_seconds: u64,
    read_timeout_seconds: u64,
}

impl Default for Document {
    fn default() -> Self {
        let config = DownloadConfig::default();
        Self {
            sources: config.sources,
            connect_timeout_seconds: config.options.connect_timeout.as_secs(),
            read_timeout_seconds: config.options.read_timeout.as_secs(),
        }
    }
}
