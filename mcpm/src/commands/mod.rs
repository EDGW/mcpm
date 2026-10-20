//! CLI command dispatch and shared presentation support.

pub(crate) mod client;
pub(crate) mod customize;
mod mod_support;
pub(crate) mod mods;
mod output_style;
pub(crate) mod server;

use anyhow::Result;

use crate::cli::Command;

pub(crate) fn run(command: Command) -> Result<()> {
    match command {
        Command::Customize(args) => customize::run(args),
        Command::Mod(args) => mods::run(args),
        Command::Client(args) => client::run(args),
        Command::Server(args) => server::run(args),
    }
}
