//! Server-scoped explicit mod caching; the file-owning host resolves selection and storage.

use super::{
    super::{mod_source::ServerMods, target},
    input::FileArgs,
};
use crate::{
    commands::source_support::SourcesArgs,
    commands::supports::mods::cache::{
        self,
        args::WriteArgs,
        identification::Completion,
        output::{DisplayArgs, Presentation},
    },
    ssh::SshArgs,
};
use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout; one JSON object per selected file). Console shows progress and a summary by default; --details adds per-file details. Remote supported through --ssh or a registered Remote workspace.\n\nITEM uses the same filename/modid/name and stdin selection as server mods info. Paths are relative to the server's mods directory; mods/ prefixes are accepted. --all selects every outer JAR. Only the server's declared loader is identified.\n\nStores whole outer JARs, including nested mods. Objects deduplicate by BLAKE3. The default mode copies; explicit hard-link modes share content protection with the source. Existing objects are reused. --metas/--meta select meta and/or ident (default both); --no-metas imports content only. Explicit selections replace defaults. --sources/--source select providers (default both); explicit selections replace defaults. Console shows separate import, meta and ident progress; provider requests and API keys stay on the client. --force refreshes requested results. File failures are reported and later files continue; completed writes are retained."
)]
pub(super) struct CacheArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    writing: WriteArgs,
    #[command(flatten)]
    sources: SourcesArgs,
    #[command(flatten)]
    display: DisplayArgs,
}

pub(super) fn run(args: CacheArgs, ssh: SshArgs) -> Result<()> {
    anyhow::ensure!(!ssh.is_requested(), "server mods cache does not support SSH yet; this command has not been migrated");
    let (path, selection) = args.files.read()?;
    if selection.is_empty() {
        return crate::commands::supports::mods::inspect::prompt_for_input();
    }
    let target = target::resolve(path, ssh)?;
    let server = ServerMods::load(target.path)?;
    let mut completion =
        Completion::new(&args.writing, &args.sources.selected()?, &target.session)?;
    let mut output = Presentation::new(args.display);
    cache::run(
        selection,
        server.scope(),
        server.runtime(),
        args.writing.options(server.loaders())?,
        || Ok(()),
        |event| completion.accept(event, &mut |event| output.event(event)),
    )?;
    completion.finish(&target.session, server.runtime(), |event| output.event(event))?;
    output.finish()
}
