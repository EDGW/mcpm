mod game;
mod identity;
mod process;
mod runtime;

use clap::Args;
use kako_mods_lib::local::mc::client::{MinecraftVersion, RuleEnvironment, launch::LaunchOptions};

#[derive(Args)]
pub(super) struct OptionArgs {
    #[command(flatten)]
    runtime: runtime::RuntimeArgs,
    #[command(flatten)]
    identity: identity::IdentityArgs,
    #[command(flatten)]
    game: game::GameArgs,
    #[command(flatten)]
    process: process::ProcessArgs,
}

impl OptionArgs {
    pub(super) fn build(
        &self,
        version: &MinecraftVersion,
        rules: &RuleEnvironment,
    ) -> anyhow::Result<LaunchOptions> {
        let mut options = LaunchOptions::new(
            self.runtime.java.clone(),
            self.identity.build(),
            rules.clone(),
        );
        self.runtime.apply(&mut options);
        self.game.apply(&mut options, version)?;
        self.process.apply(&mut options);
        Ok(options)
    }
}
