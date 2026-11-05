//! Passes server-owned selections to the complete Host identification operation.

use anyhow::Result;
use clap::Args;

use super::super::input::FileArgs;
use crate::{
    commands::{execution::Runtime, source_support::identify},
    ssh::SshArgs,
};
use mcpm_backend::operations::mods::loading::{ScopeInput, selection::SelectionInput};

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: Console (field groups) and Raw (TSV; --raw or redirected stdout); Remote supported through --ssh.\n\nWith --ssh, server selection, file reads, cache reuse and source requests run on the selected host.\nInputs match filename, then exact modid, then exact mod name in the server's mods directory.\nPaths are relative to that directory; mods/ prefixes are accepted. Use --all to select every mod.\nRedirected stdin appends one selection per line; --input sets its interpretation.\nMatching uses file contents and is independent of Minecraft version or loader compatibility.\n--columns, --column and -C select fields; --headers adds a Raw header.\nOptional Raw columns query_status, cache_coverage and origin describe Host query results.\n\nExamples:\n  {server} mods source identify --server ./my-server --source modrinth --all\n  {server} mods source identify create.jar --source curseforge\n  {server} mods source identify --modid create --source modrinth --columns path,project,id,status\n  printf '%s\\n' create | {server} mods source identify --source modrinth --input modid", server = crate::program::roots().server)
)]
pub(super) struct IdentifyArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    options: identify::IdentifyOptions,
}

pub(super) fn run(args: IdentifyArgs, ssh: SshArgs) -> Result<()> {
    let (server, selection) = args.files.read()?;
    if selection.is_empty() {
        crate::commands::supports::mods::inspect::prompt_for_input()?;
        return Ok(());
    }
    let runtime = Runtime::new()?;
    let mut scope = ScopeInput::Server {
        path: server,
        no_cache: args.options.cache.no_cache,
    };
    let executor = runtime.block_on(crate::commands::supports::mods::target::executor(
        &mut scope, ssh,
    ))?;
    identify::run_selection_at(
        args.options,
        scope,
        SelectionInput::from_selection(selection)?,
        &executor,
    )
}
