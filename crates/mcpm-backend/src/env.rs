//! Process-local environments handed to operations.
//!
//! A host operation only ever receives [`HostEnv`], so credentials and providers are
//! unreachable from code that may run on another machine. Surface operations receive
//! [`SurfaceEnv`], which owns the HTTP client used to construct providers.

use std::time::Duration;

use anyhow::{Context, Result};
use kako_mc_lib::source::Source;

use crate::source::provider;

/// Environment of an operation that always runs in the calling process.
///
/// Surface operations may build providers from the specifications in their input; the
/// credentials stay in that input and never reach a host environment.
#[derive(Clone, Debug)]
pub struct SurfaceEnv {
    client: reqwest::Client,
}

impl SurfaceEnv {
    /// Builds the shared HTTP client used by every provider of this process.
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build the source HTTP client")?;
        Ok(Self { client })
    }

    /// The shared provider HTTP client.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Builds one configured provider from a source specification.
    pub fn source(&self, specification: &str) -> Result<Box<dyn Source>> {
        provider::build(&self.client, specification)
    }
}

/// Environment of an operation that may run on a remote host.
///
/// It deliberately carries nothing: host operations resolve every resource from their
/// input and the filesystem, and therefore cannot reach providers or credentials.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostEnv;
