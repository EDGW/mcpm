pub(crate) mod client;
pub(crate) mod mods;
mod output_style;

use anyhow::Result;

use crate::cli::Command;

pub(crate) fn run(command: Command) -> Result<()> {
    match command {
        Command::Mod(args) => mods::run(args),
        Command::Client(args) => client::run(args),
    }
}
