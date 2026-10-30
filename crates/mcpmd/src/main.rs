//! `mcpmd` is the small remote process entrypoint for serving and linking operations.
//!
//! Business operations remain in `mcpm-backend`; this binary only builds the runtime, selects
//! a subcommand and delegates lifecycle work to `mcpm-exec`.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod link;
mod serve;

#[derive(Debug, Parser)]
#[command(name = "mcpmd", version, about = "mcpm remote operation process")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve host operations on the protocol stream.
    Serve,
    /// Hold one SSH serve session behind a local IPC socket.
    Link(link::Args),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_ansi(false).with_writer(std::io::stderr).init();
    let cli = Cli::parse();
    match cli.command {
        Command::Serve => serve::run(),
        Command::Link(args) => link::run(args),
    }
}
