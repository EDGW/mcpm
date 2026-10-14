pub(crate) mod mods;

use anyhow::Result;

use crate::cli::Command;

pub(crate) fn run(command: Command) -> Result<()> {
    match command {
        Command::Mod(args) => mods::run(args),
    }
}
