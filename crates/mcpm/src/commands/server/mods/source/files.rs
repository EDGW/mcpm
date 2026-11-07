//! Sends registered server context to Host file browsing.

use anyhow::Result;
use clap::Args;

use crate::commands::source_support::files;

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout); Remote supported through --ssh.\n\nPROJECT accepts remote project IDs or exact slugs; redirected stdin appends one per line.\nWith --ssh, --server and source requests run on the selected host; the UI remains local.\nMinecraft version and loader default to the registered server; explicit filters override each default.\nUse --all-versions to disable automatic defaults without clearing explicit filters. Multiple projects automatically use list output.\n\nExamples:\n  {server} mods source files create --server ./my-server --source modrinth\n  {server} mods source files 688231 --source curseforge --all-versions\n  {server} mods source files create --source modrinth -N --columns id,filename\n  printf '%s\\n' create ZzjhlDgM | {server} mods source files --source modrinth", server = crate::program::roots().server)
)]
pub(super) struct ServerFilesArgs {
    #[command(flatten)]
    server: super::CompatibilityArgs,
    #[command(flatten)]
    files: files::FilesArgs,
}

pub(super) fn run(args: ServerFilesArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let runtime = crate::commands::execution::Runtime::new()?;
    let (executor, path, _) =
        runtime.block_on(ssh.create_executor(Some(args.server.path.clone())))?;
    let context = args.server.context(path.expect("server source path"));
    files::run_at(args.files, &executor, Some(context))
}
