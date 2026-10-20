//! CLI command dispatch and shared presentation support.

pub(crate) mod client;
pub(crate) mod customize;
pub(crate) mod debug;
mod mod_support;
pub(crate) mod mods;
mod output_style;
pub(crate) mod server;
mod source_support;

use anyhow::Result;

use crate::cli::Command;

pub(crate) fn run(command: Command) -> Result<()> {
    match command {
        Command::Customize(args) => customize::run(args),
        Command::Debug(args) => debug::run(args),
        Command::Mod(args) => mods::run(args),
        Command::Client(args) => client::run(args),
        Command::Server(args) => server::run(args),
    }
}
