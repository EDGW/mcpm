//! Ordered batch refresh of independent Loader and source snapshots on cached bytes.

use std::io::Write;

use anyhow::Result;
use clap::{Args, Subcommand};
use mcpm_backend::local::cache::mods::{MetadataRefresh, ModsCacheSelection};

use super::{batch::Parallelism, input::Inputs};

mod ident;
mod meta;
mod output;
use output::Presentation;

#[derive(Debug, Args)]
#[command(
    subcommand_precedence_over_arg = true,
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout; silent on success, exit code 0). Local only.\n\nPlace BLAKE3 IDs before meta or ident. Redirected stdin appends one ID per nonblank line. Duplicate IDs are processed once in first-seen order. --all selects the complete cache inventory in BLAKE3 order and conflicts with HASH arguments or nonblank stdin. An empty --all selection succeeds. With --all, --parallelism COUNT limits concurrent entries (default 10; positive integers only). Use 1 for sequential work. Explicit HASH inputs stay sequential. Results retain BLAKE3 order.\nConsole defaults to one-line BLAKE3 and success/failed results when more than three entries are selected; otherwise it shows BLAKE3, Status and File followed by full details. --short and --details force either format and conflict with each other. These flags and --all may appear before or after meta/ident. --headers has no effect.\nA failed entry stops new work beyond it; in-flight operations finish; completed writes remain. Errors are reported to stderr with their cause chain and a nonzero exit code."
)]
pub(super) struct RefreshArgs {
    #[command(flatten)]
    inputs: Inputs,
    #[command(flatten)]
    parallelism: Parallelism,
    /// Reidentify selected results even when current cached results exist.
    #[arg(long, global = true)]
    force: bool,
    /// Show full Console details even when more than three entries are selected.
    #[arg(long, global = true, conflicts_with = "short")]
    details: bool,
    /// Show only BLAKE3 and success/failed on one Console line per entry.
    #[arg(long, global = true, conflicts_with = "details")]
    short: bool,
    #[command(subcommand)]
    command: RefreshCommand,
}

#[derive(Debug, Subcommand)]
enum RefreshCommand {
    /// Refresh Loader metadata (Console, Raw); defaults to forge,fabric,neoforge.
    Meta(meta::MetaArgs),
    /// Refresh source identification (Console, Raw); defaults to modrinth,curseforge.
    Ident(ident::IdentArgs),
}

pub(super) fn run(
    args: RefreshArgs,
    selection: ModsCacheSelection,
    out: &mut impl Write,
) -> Result<()> {
    let inputs = args.inputs.read()?;
    let parallelism = args.parallelism.selected(inputs.is_all());
    let cache = selection.open()?;
    let ids = inputs.resolve(&cache)?;
    let mode = if args.force {
        MetadataRefresh::Force
    } else {
        MetadataRefresh::MissingOrOutdated
    };
    let mut output = Presentation::new(out, &cache, ids.len(), args.short, args.details);
    match args.command {
        RefreshCommand::Meta(args) => meta::run(args, ids, &cache, mode, parallelism, &mut output),
        RefreshCommand::Ident(args) => {
            ident::run(args, ids, &cache, mode, parallelism, &mut output)
        }
    }
}
