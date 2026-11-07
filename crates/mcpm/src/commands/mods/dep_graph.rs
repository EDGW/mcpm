//! Builds graph requests without opening or normalizing Host directory inputs.

use crate::{
    commands::{
        execution::Runtime,
        supports::mods::{
            dep_graph::{self, BuildOptions},
            loaders::LoaderArgs,
        },
    },
    ssh::SshArgs,
};
use anyhow::Result;
use clap::Args;
use mcpm_backend::operations::mods::loading::ScopeInput;
use std::path::PathBuf;
pub(super) type DepGraphArgs = dep_graph::DepGraphArgs<BuildArgs>;
#[derive(Args, Debug)]
pub(super) struct BuildArgs {
    /// Host directory containing .jar and .jar.disabled files (not scanned recursively).
    #[arg(value_name = "DIR")]
    directory: PathBuf,
    #[command(flatten)]
    loaders: LoaderArgs,
    #[command(flatten)]
    options: BuildOptions,
}
pub(super) fn run(args: DepGraphArgs, ssh: SshArgs) -> Result<()> {
    Runtime::new()?.block_on(args.run(ssh, |args, ssh| async move {
        let mut scope = ScopeInput::Directory {
            directory: args.directory,
            loaders: args.loaders.ids(),
            cache: None,
        };
        let executor = crate::commands::supports::mods::target::executor(&mut scope, ssh).await?;
        args.options.run(scope, &executor).await
    }))
}
