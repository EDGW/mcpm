//! CLI command dispatch and shared presentation support.

pub(crate) mod client;
pub(crate) mod customize;
pub(crate) mod debug;
mod execution;
mod mod_support;
pub(crate) mod mods;
mod output_style;
mod progress;
pub(crate) mod server;
mod source_support;
mod stdin;

use anyhow::Result;

use crate::cli::Command;

pub(crate) fn run(command: Command, ssh: crate::ssh::SshArgs) -> Result<()> {
    if ssh.is_requested() && !matches!(command, Command::Mod(_)) {
        anyhow::bail!(
            "--ssh and --ssh-mcpm are currently supported only by mcpm mod list and mod source identify"
        );
    }
    match command {
        Command::Remote(args) => {
            let registry = crate::remote::registry::registered();
            crate::remote::server::run(args.max_concurrency, registry)
        }
        Command::Customize(args) => customize::run(args),
        Command::Debug(args) => debug::run(args),
        Command::Mod(args) => mods::run(args, ssh),
        Command::Client(args) => client::run(args),
        Command::Server(args) => server::run(args),
    }
}
