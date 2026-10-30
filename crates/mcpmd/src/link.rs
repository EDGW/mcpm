//! `mcpmd link` delegates connection-holder lifecycle to `mcpm-exec`.

use std::time::Duration;
use anyhow::Result;
use clap::Args as ClapArgs;
use mcpm_exec::link::Options;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// SSH command including its destination, for example `ssh host`.
    #[arg(long)]
    ssh: String,
    /// Remote mcpmd executable.
    #[arg(long, default_value = "mcpmd")]
    executable: String,
    /// Build identity included in the holder identity.
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    build: String,
    /// Idle timeout after all clients and calls are gone.
    #[arg(long, default_value_t = 300)]
    idle_timeout: u64,
}

pub fn run(args: Args) -> Result<()> {
    mcpm_exec::link::run(Options { ssh: args.ssh, executable: args.executable, build: args.build, idle_timeout: Duration::from_secs(args.idle_timeout) })
        .map_err(anyhow::Error::from)
}
