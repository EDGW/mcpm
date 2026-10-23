//! Server-scoped explicit mod caching; the file-owning host resolves selection and storage.

use super::{
    super::{
        mod_source::ServerMods,
        target::{self, Target},
    },
    input::FileArgs,
};
use crate::{
    commands::mod_support::caching::{
        self,
        args::WriteArgs,
        output::{DisplayArgs, Presentation},
    },
    ssh::SshArgs,
};
use anyhow::Result;
use clap::Args;

mod remote;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout; one JSON object per selected file). Console shows progress and a summary by default; --expanded adds per-file details. Remote supported through --ssh or a registered Remote workspace.\n\nITEM uses the same filename/modid/name and stdin selection as server mods info. Paths are relative to the server's mods directory; mods/ prefixes are accepted. --all selects every outer JAR. Only the server's declared loader is identified.\n\nStores whole outer JARs, including nested mods. Objects deduplicate by BLAKE3. The default mode copies; explicit hard-link modes share content protection with the source. Existing objects are reused. --force reparses the declared loader. File failures are reported and later files continue; completed writes are retained."
)]
pub(super) struct CacheArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    writing: WriteArgs,
    #[command(flatten)]
    display: DisplayArgs,
}

pub(super) fn run(args: CacheArgs, ssh: SshArgs) -> Result<()> {
    let (path, selection) = args.files.read()?;
    if selection.is_empty() {
        return crate::commands::mod_support::inspect::prompt_for_input();
    }
    let Target { path, ssh, .. } = target::resolve(path, ssh)?;
    if ssh.is_requested() {
        return remote::run(ssh, path, selection, args.writing, args.display);
    }
    let server = ServerMods::load(path)?;
    let mut output = Presentation::new(args.display);
    caching::run(
        selection,
        server.scope(),
        args.writing.options(server.loaders())?,
        || Ok(()),
        |event| output.event(event),
    )?;
    output.finish()
}
