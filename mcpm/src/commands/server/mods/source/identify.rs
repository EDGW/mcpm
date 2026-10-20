//! Resolves local server mod selections before remote byte identification.

use anyhow::{Result, ensure};
use clap::Args;

use crate::commands::{mod_support::selection, source_support::identify};

use super::super::input::FileArgs;

#[derive(Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout).\n\nInputs match filename, then exact modid, then exact mod name in the server's mods directory.\nPaths are relative to that directory; mods/ prefixes are accepted. Use --all to select every mod.\nRedirected stdin appends one selection per line; --input sets its interpretation.\nMatching uses file contents and is independent of Minecraft version or loader compatibility.\n\nExamples:\n  mcpm server mods source identify --server ./my-server --source modrinth --all\n  mcpm server mods source identify create.jar --source curseforge\n  mcpm server mods source identify --modid create --source modrinth --columns path,project,id,status\n  printf '%s\\n' create | mcpm server mods source identify --source modrinth --input modid"
)]
pub(super) struct IdentifyArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    options: identify::IdentifyOptions,
}

pub(super) fn run(args: IdentifyArgs) -> Result<()> {
    let Some((server, selection)) = args.files.with_stdin()?.resolve()? else {
        return Ok(());
    };
    let (paths, failed) = selection::resolve_paths(selection, server.scope(), server.loaders())?;
    if !paths.is_empty() {
        identify::run_paths(args.options, paths)?;
    }
    ensure!(
        failed == 0,
        "failed to resolve {failed} server mod selection(s)"
    );
    Ok(())
}
