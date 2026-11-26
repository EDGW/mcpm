//! Shared download-source selection and second-based timeout arguments.

use std::{str::FromStr, time::Duration};

use anyhow::{Result, anyhow, ensure};
use clap::Args;
use mcpm_lib::download::{DownloadConfig, DownloadOptions};

#[derive(Args, Debug, Default)]
pub(crate) struct SourceArgs {
    /// Ordered download sources, comma-separated; replaces bmclapi,official rather than appending.
    #[arg(
        long = "source",
        value_delimiter = ',',
        value_name = "SOURCE1,SOURCE2,..."
    )]
    sources: Option<Vec<String>>,
    /// Positive seconds: CONNECT or CONNECT:READ. With CONNECT alone, READ is three times CONNECT.
    #[arg(long = "source-timeout", value_name = "CONNECT[:READ]")]
    timeout: Option<SourceTimeout>,
}

impl SourceArgs {
    /// Resolves CLI overrides once, using the library's sole configuration validation path.
    pub(crate) fn download_config(&self) -> Result<DownloadConfig> {
        let defaults = DownloadConfig::default();
        let sources = match &self.sources {
            Some(values) => values.iter().map(|value| value.trim().to_owned()).collect(),
            None => defaults.sources().to_vec(),
        };
        let options = self
            .timeout
            .as_ref()
            .map(|value| DownloadOptions {
                connect_timeout: Duration::from_secs(value.connect),
                read_timeout: Duration::from_secs(value.read),
            })
            .unwrap_or_else(|| defaults.options());
        DownloadConfig::new(sources, options)
    }
}

#[derive(Debug, Clone)]
struct SourceTimeout {
    connect: u64,
    read: u64,
}

impl FromStr for SourceTimeout {
    type Err = String;

    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        fn seconds(value: &str) -> Result<u64> {
            ensure!(
                !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
                "timeouts must be positive integer seconds"
            );
            let seconds = value.parse::<u64>()?;
            ensure!(seconds > 0, "timeouts must be positive integer seconds");
            Ok(seconds)
        }
        let parse = || -> Result<Self> {
            let (connect, read) = match input.split_once(':') {
                Some((connect, read)) => (seconds(connect)?, seconds(read)?),
                None => {
                    let connect = seconds(input)?;
                    (
                        connect,
                        connect
                            .checked_mul(3)
                            .ok_or_else(|| anyhow!("read timeout (3 * CONNECT) overflows"))?,
                    )
                }
            };
            Ok(Self { connect, read })
        };
        parse().map_err(|error| error.to_string())
    }
}

