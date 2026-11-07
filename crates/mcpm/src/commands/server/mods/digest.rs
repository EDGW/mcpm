//! Server-scoped archive selection over the shared execution-host digest operation.

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::operations::mods::{digest::DigestTarget, loading::selection::SelectionInput};

use super::input::FileArgs;
use crate::{
    commands::{
        execution::Runtime,
        supports::mods::{digest, inspect},
    },
    ssh::SshArgs,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Supply selections as positional inputs (like mods info), explicit --file/--modid/--name options, or one per nonblank stdin line; --input sets stdin interpretation. Automatic inputs match filename, then exact modid, then exact name. Use --all for the complete mods directory; an empty selection does not scan it.\n\n--server selects a registered server (default: .). File inputs are relative to its mods directory; mods/ prefixes are accepted, absolute paths and parent traversal are rejected. Explicit --ssh selects the remote host; otherwise Local/Remote workspace registration selects execution through mcpmd. Files stay on that host.\n\nSize and SHA-1 are always computed; --murmur2 adds the CurseForge-compatible fingerprint. File selections and --all do not parse Mod metadata; ID/name matching uses the declared loader. Nested matches select their entire outer archive. Each physical file is output once, in selection order (path order for --all). No cache access or source queries.\n\nResults continue after individual selection or file errors; any failure causes a nonzero exit. --parallelism bounds indexing and file reads (default: 10). Raw TSV columns are file, size, sha1, murmur2; unrequested or unavailable values are blank. --headers adds the Raw column names."
)]
pub(super) struct DigestArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    options: digest::Options,
}

pub(super) fn run(args: DigestArgs, ssh: SshArgs) -> Result<()> {
    let (server, selection) = args.files.read()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let selection = SelectionInput::from_selection(selection)?;
    Runtime::new()?.block_on(async {
        let (executor, path, _) = ssh.create_executor(Some(server)).await?;
        digest::run(
            &executor,
            DigestTarget::Server {
                path: path.context("server mod digest requires a path")?,
                selection,
            },
            args.options,
        )
        .await
    })
}
