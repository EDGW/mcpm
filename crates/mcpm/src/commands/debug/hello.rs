//! Render the welcome message and common server-management commands.

use std::io;

use anyhow::Result;
use mcpm_rich::rich;

use crate::{
    build_info::VERSION,
    display::{console::ConsoleWriter, rich::block, style::OutputStyle},
};

pub(super) fn run() -> Result<()> {
    let guide = rich!(
        "
        [Welcome to mcpm {VERSION}!](primary)

        [Usage:](primary)

        [Create a new Minecraft server with](important) `mcpm new <PATH>`

        - Use `mcpm new <PATH> --version 1.20.1` to create a vanilla 1.20.1 server.
        - Use `mcpm new <PATH> --version 1.20.1 --loader neoforge[:<version>]` to create a 1.20.1 NeoForge server.
        - Add the `--eula` flag to read and accept the EULA. mcpm cannot accept the EULA on your behalf; you must accept it [yourself](important).
        - If `<PATH>` is omitted, it defaults to the current working directory.
        - Run `mcpm new --help` to see more options.

        [Migrate from an existing server with](important) `mcpm init <PATH>`

        - This command has many options; run `mcpm init --help` to view them.
        - After migrating, edit `conf.toml` to change mutable settings, and edit `launch.toml` to change launch settings.

        [Launch an mcpm server with](important) `mcpm launch`

        - Use `mcpm launch` to start the mcpm server with default arguments.
        - Use `mcpm launch -j <PATH> -m 1G:4G -A <WEBSITE>` to specify the Java path, memory allocation, and authlib-injector authentication URL. These arguments override `conf.toml`.
        - Run `mcpm launch --help` to see more options.

        Use `mcpm-tools` to access the full toolset, and `mcpm` or `mcpm-server` to invoke mcpm's server management tool.
        "
    );
    block(
        &mut ConsoleWriter::new(&mut io::stdout().lock()),
        &guide,
        &OutputStyle::new(),
    )?;
    Ok(())
}
