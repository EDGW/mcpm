//! Daemon entry point: stdio operation serving and reusable per-user SSH links.

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

mod link;
mod logs;
mod serve;

#[derive(Parser)]
#[command(name = "mcpmd", version = env!("MCPM_BUILD_VERSION"), about = "Serve operations or manage reusable SSH links")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Execute registered operations on stdin/stdout; stdout is reserved for protocol frames.
    Serve,
    /// Start, inspect, or stop per-user SSH links.
    Link(LinkArgs),
}

#[derive(Args)]
struct LinkArgs {
    #[command(subcommand)]
    command: Option<LinkCommand>,
    /// Use tab-separated output even in a terminal.
    #[arg(long, global = true)]
    raw: bool,
    /// SSH connection command for a foreground link.
    #[arg(value_name = "SSH")]
    ssh: Option<String>,
    /// Remote mcpmd executable; defaults to mcpmd on PATH.
    #[arg(long)]
    mcpmd: Option<String>,
    /// Exit after this many seconds without clients or calls; zero disables idle exit.
    #[arg(long, default_value_t = 600)]
    idle_timeout: u64,
    #[arg(long, hide = true)]
    connection: Option<String>,
    #[arg(long, hide = true)]
    ready: bool,
}

#[derive(Subcommand)]
enum LinkCommand {
    /// Query live links and remove stale state records (Console and Raw TSV).
    List,
    /// Stop accepting work and drain calls; --force aborts immediately.
    Stop {
        ssh: Option<String>,
        #[arg(long, conflicts_with = "ssh")]
        all: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        mcpmd: Option<String>,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let result: Result<()> = match cli.command {
        Command::Serve => serve::run().await,
        Command::Link(args) => link::run(args).await,
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let mut causes = error.chain();
            if let Some(error) = causes.next() {
                eprintln!("error: {error}");
            }
            for cause in causes {
                eprintln!("  caused by: {cause}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}
