//! Literal file digest CLI over the shared execution-host summary flow.

use std::path::PathBuf;

use anyhow::{Result, ensure};
use clap::Args;
use mcpm_backend::operations::mods::digest::DigestTarget;

use crate::{
    commands::{execution::Runtime, stdin, supports::mods::digest},
    ssh::SshArgs,
};

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console (default in a terminal), Raw (--raw or redirected stdout). Supply files as arguments, one per nonblank stdin line, or both; at least one file is required. Paths are literal: spaces are preserved and stdin quotes are not interpreted.\n\nFiles are read on the execution host selected by --ssh, or locally without --ssh. No upload, Mod parsing, cache access or source query is performed. Size and SHA-1 are always computed; --murmur2 adds the CurseForge-compatible fingerprint. Duplicate paths share digest work but retain their original output positions. Results are printed in input order, continuing after individual failures; any failure causes a nonzero exit. Raw TSV columns are file, size, sha1, murmur2; unrequested or unavailable values are blank."
)]
pub(super) struct DigestArgs {
    /// Files; redirected stdin appends one literal file path per nonblank line.
    #[arg(value_name = "FILE", num_args = 0..)]
    files: Vec<PathBuf>,
    #[command(flatten)]
    options: digest::Options,
}

pub(super) fn run(args: DigestArgs, ssh: SshArgs) -> Result<()> {
    let paths = stdin::append(args.files)?;
    ensure!(
        !paths.is_empty(),
        "provide files as arguments or one per stdin line"
    );
    let runtime = Runtime::new()?;
    runtime.block_on(async {
        let (executor, _, _) = ssh.create_executor(None).await?;
        digest::run(&executor, DigestTarget::Paths { paths }, args.options).await
    })
}
