//! Cache CLI flows over Executor; file operations and provider requests run on Host.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::Result;
use clap::{Args, Subcommand};
use mcpm_backend::operations::cache::CacheInput;
use mcpm_exec::service::Executor;
use mcpm_lib::local::cache::mods::ModsCacheSelection;
use tokio::sync::OnceCell;

use crate::{commands::execution::Runtime, ssh::SshArgs};

mod batch;
mod columns;
mod export;
mod import;
mod info;
mod init;
mod input;
mod list;
mod rebuild_index;
mod refresh;
mod remove;
mod verify;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout). Raw list, info and simple operations use TSV; refresh succeeds silently.\n\nDefault directory: ~/.mcpm/cache/mods on the selected host. --ssh selects remote cache management, including metadata refresh and provider requests. Import and export paths belong to the selected host, not the calling computer. Import stores content only; info/list read saved metadata without identifying or requesting providers. Verify checks content only. Exports never overwrite or create parent directories. No automatic eviction.\n\nCommands that modify a server stash cache are refused, including verify --autoremove. Do not manage stash caches manually: snapshots depend on their contents. --no-protect-stash bypasses this protection. Read-only list, info, verify and exports outside stash caches remain available. remove --all requires confirmation; use --yes in Raw or non-interactive mode."
)]
pub(crate) struct CacheArgs {
    /// Cache root; explicit directories must already be initialized.
    #[arg(long, global = true, value_name = "PATH")]
    directory: Option<PathBuf>,
    /// Allow modifying a server's stash cache; this can make stashes unusable.
    #[arg(long, global = true)]
    no_protect_stash: bool,
    /// Include a header in Raw TSV output; ignored by Console and refresh.
    #[arg(long, global = true)]
    headers: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize a missing or empty cache directory (Console, Raw).
    Init,
    /// Import content without calculating meta or ident (Console, Raw).
    Import(import::ImportArgs),
    /// List BLAKE3 IDs or selected cache columns (Console, Raw).
    List(list::ListArgs),
    /// Inspect cached content and named metadata (Console, Raw).
    Info(info::InfoArgs),
    /// Refresh meta or ident metadata (Console, Raw).
    Refresh(refresh::RefreshArgs),
    /// Verify cached content without reading metadata (Console, Raw).
    Verify(verify::VerifyArgs),
    /// Export one object's bytes to multiple destinations (Console, Raw).
    Export(export::ExportArgs),
    /// Remove selected or all cache-owned names; exported hard links remain (Console, Raw).
    Remove(remove::RemoveArgs),
    /// Rebuild the optional imohash lookup index (Console, Raw).
    RebuildIndex,
}

pub(crate) fn run(args: CacheArgs, ssh: SshArgs) -> Result<()> {
    let selection = match args.directory {
        Some(path) => ModsCacheSelection::Directory { path },
        None => ModsCacheSelection::Default,
    };
    let mut out = io::stdout().lock();
    let command = args.command;
    let target = if ssh.is_requested() {
        match ssh.destination() {
            Some(destination) => format!(
                "remote host \"{}\"",
                crate::display::style::text(&destination)
            ),
            None => "the selected remote host".into(),
        }
    } else {
        "this computer".into()
    };
    let context = Context {
        cache: CacheInput {
            selection,
            no_protect_stash: args.no_protect_stash,
        },
        headers: args.headers,
        target,
        ssh,
        executor: OnceCell::new(),
    };
    Runtime::new()?.block_on(async {
        match command {
            Command::Init => init::run(&context, &mut out).await?,
            Command::Import(args) => import::run(args, &context, &mut out).await?,
            Command::List(args) => list::run(args, &context, &mut out).await?,
            Command::Info(args) => info::run(args, &context, &mut out).await?,
            Command::Verify(args) => verify::run(args, &context, &mut out).await?,
            Command::Export(args) => export::run(args, &context, &mut out).await?,
            Command::Remove(args) => remove::run(args, &context, &mut out).await?,
            Command::RebuildIndex => rebuild_index::run(&context, &mut out).await?,
            Command::Refresh(args) => refresh::run(args, &context, &mut out).await?,
        }
        out.flush().map_err(Into::into)
    })
}

/// Resolve execution only after each leaf has validated its input; reuse one connection.
struct Context {
    cache: CacheInput,
    headers: bool,
    target: String,
    ssh: SshArgs,
    executor: OnceCell<Executor>,
}
impl Context {
    async fn executor(&self) -> Result<&Executor> {
        self.executor
            .get_or_try_init(|| async {
                let (executor, _, _) = self.ssh.create_executor(None).await?;
                Ok(executor)
            })
            .await
    }
}
