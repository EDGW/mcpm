//! Game launch arguments, memory, window and multiplayer options.

use std::path::PathBuf;

use clap::Args;
use kako_mods_lib::local::mc::client::{
    MinecraftVersion,
    launch::{LaunchOptions, LoggingOptions, QuickPlay, QuickPlayTarget, ServerOptions},
};

#[derive(Args, Debug)]
#[command(next_help_heading = "Game")]
pub(super) struct GameArgs {
    /// Game directory; defaults to each version's directory.
    #[arg(long, conflicts_with = "shared_game_directory")]
    game_directory: Option<PathBuf>,
    /// Use the Minecraft root as game directory for all selected versions.
    #[arg(long)]
    shared_game_directory: bool,
    /// Process working directory; defaults to game directory.
    #[arg(long)]
    working_directory: Option<PathBuf>,
    /// Initial window width; requires --height.
    #[arg(long, requires = "height", value_parser = clap::value_parser!(u32).range(1..))]
    width: Option<u32>,
    /// Initial window height; requires --width.
    #[arg(long, requires = "width", value_parser = clap::value_parser!(u32).range(1..))]
    height: Option<u32>,
    /// Start in fullscreen mode.
    #[arg(long)]
    fullscreen: bool,
    /// Enable demo mode.
    #[arg(long)]
    demo: bool,
    /// Connect using the legacy --server argument.
    #[arg(long, conflicts_with_all = ["quick_play_singleplayer", "quick_play_multiplayer", "quick_play_realms"])]
    server: Option<String>,
    /// Legacy server port.
    #[arg(long, requires = "server", value_parser = clap::value_parser!(u16).range(1..))]
    port: Option<u16>,
    /// Open a world using profile-declared Quick Play arguments.
    #[arg(long, conflicts_with_all = ["quick_play_multiplayer", "quick_play_realms"])]
    quick_play_singleplayer: Option<String>,
    /// Connect to a server using profile-declared Quick Play arguments.
    #[arg(long, conflicts_with = "quick_play_realms")]
    quick_play_multiplayer: Option<String>,
    /// Connect to a Realm using profile-declared Quick Play arguments.
    #[arg(long)]
    quick_play_realms: Option<String>,
    /// Override shared cache locations (relative paths resolve against the root).
    #[arg(long)]
    libraries_directory: Option<PathBuf>,
    /// Override the assets cache directory.
    #[arg(long)]
    assets_directory: Option<PathBuf>,
    /// Override the resolved client JAR.
    #[arg(long)]
    client_jar: Option<PathBuf>,
    /// Natives directory; defaults to versions/<version>/<version>-natives.
    #[arg(long)]
    natives_directory: Option<PathBuf>,
    /// Use an existing natives directory without extracting archives.
    #[arg(long, requires = "natives_directory")]
    use_existing_natives: bool,
    /// Disable the manifest's logging configuration argument.
    #[arg(long)]
    no_logging_config: bool,
}

impl GameArgs {
    pub(super) fn apply(
        &self,
        options: &mut LaunchOptions,
        version: &MinecraftVersion,
    ) -> anyhow::Result<()> {
        options.directories.game = if self.shared_game_directory {
            None
        } else if let Some(path) = &self.game_directory {
            Some(path.clone())
        } else {
            Some(version.path().canonicalize()?)
        };
        options.directories.working = self.working_directory.clone();
        options.directories.libraries = self.libraries_directory.clone();
        options.directories.assets = self.assets_directory.clone();
        options.directories.client_jar = self.client_jar.clone();
        options.window.width = self.width;
        options.window.height = self.height;
        options.window.fullscreen = self.fullscreen;
        options.window.demo = self.demo;
        options.server = self.server.as_ref().map(|address| ServerOptions {
            address: address.clone(),
            port: self.port,
        });
        let target = self
            .quick_play_singleplayer
            .clone()
            .map(QuickPlayTarget::Singleplayer)
            .or_else(|| {
                self.quick_play_multiplayer
                    .clone()
                    .map(QuickPlayTarget::Multiplayer)
            })
            .or_else(|| self.quick_play_realms.clone().map(QuickPlayTarget::Realms));
        options.quick_play = target.map(|target| QuickPlay {
            target,
            log_path: None,
        });
        options.natives.directory = self.natives_directory.clone();
        options.natives.extract = !self.use_existing_natives;
        if self.no_logging_config {
            options.logging = LoggingOptions::Disabled;
        }
        Ok(())
    }
}
