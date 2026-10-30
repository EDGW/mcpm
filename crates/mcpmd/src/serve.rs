//! Remote `serve` command: construct the backend registry and delegate protocol handling.

use anyhow::{Context, Result};
use mcpm_backend::{register_host_operations, Providers, SurfaceEnv};

/// Runs the protocol v6 host-operation loop on stdin/stdout.
pub fn run() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build mcpmd runtime")?;
    runtime.block_on(async {
        let providers = Providers::new(SurfaceEnv::new()?);
        let registry = register_host_operations(providers)?;
        mcpm_exec::serve::run(registry, env!("CARGO_PKG_VERSION"))
            .await
            .map_err(anyhow::Error::from)
    })
}
