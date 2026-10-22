//! Source-identification CLI options, ordered presentation and shared local acquisition.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Result, ensure};
use clap::Args;
use kako_mods_lib::source::SourceId;
use std::num::NonZeroUsize;

mod columns;
mod model;
mod output;
mod remote;
pub(in crate::commands) mod runtime;
mod summary;
mod worker;

pub(in crate::commands) use model::{DEFAULT_PARALLELISM, Event, Outcome};
pub(in crate::commands) use worker::Identification;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout); Remote supported through --ssh.\n\nPaths are literal file paths, resolved on the remote host with --ssh. Redirected stdin is appended, one path per line.\nUnmatched files are reported as unmatched; read or request failures exit unsuccessfully.\n\nExamples:\n  mcpm mod source identify --source modrinth ./mods/create.jar\n  mcpm mod source identify --source curseforge ./mods/a.jar ./mods/b.jar\n  mcpm mod source identify --source 'curseforge@KEY' ./mods/create.jar\n  printf '%s\\n' ./mods/a.jar ./mods/b.jar | mcpm mod source identify --source modrinth\n  mcpm --raw mod source identify --source modrinth ./mods/create.jar --columns path,project,id,status"
)]
pub(in crate::commands) struct IdentifyArgs {
    /// Files to identify (remote paths with --ssh) by their contents, not their names.
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    options: IdentifyOptions,
}

/// Source and presentation options, independent of local path selection.
#[derive(Args)]
pub(in crate::commands) struct IdentifyOptions {
    /// modrinth, curseforge (Curse.tools), or curseforge@KEY (official API).
    #[arg(long, value_name = "SOURCE")]
    source: String,
    /// Maximum simultaneous file lookups; results retain input order.
    #[arg(long, default_value_t = NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap(), value_name = "COUNT")]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    columns: columns::ColumnArgs,
}

impl std::fmt::Debug for IdentifyOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentifyOptions")
            .field(
                "source",
                &self.source.split('@').next().unwrap_or(&self.source),
            )
            .field("parallelism", &self.parallelism)
            .field("columns", &self.columns)
            .finish()
    }
}

/// Where the input bytes live; remote acquisition reuses a caller-owned session.
#[derive(Debug, Clone, Copy)]
pub(in crate::commands) enum Location<'a> {
    Local,
    Remote(&'a crate::remote::client::Session),
}
impl Location<'_> {
    pub(in crate::commands) fn is_local(self) -> bool {
        matches!(self, Self::Local)
    }
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands) fn run(args: IdentifyArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    ssh.validate()?;
    let paths = crate::commands::mod_support::stdin::append(args.paths)?;
    if !ssh.is_requested() {
        return run_paths(args.options, paths);
    }
    ensure!(
        !paths.is_empty(),
        "provide file paths as arguments or one per stdin line"
    );
    let _span = tracing::info_span!("source_identify", source = "ssh").entered();
    let mut session = crate::remote::client::Session::start(&mut ssh.command()?)?;
    let result = present(args.options, paths, Some(&session));
    session.close()?;
    result
}

/// Identifies already-resolved paths without reading stdin again.
pub(in crate::commands) fn run_paths(options: IdentifyOptions, paths: Vec<PathBuf>) -> Result<()> {
    let _span = tracing::info_span!("source_identify", source = "local").entered();
    present(options, paths, None)
}

fn present(
    options: IdentifyOptions,
    paths: Vec<PathBuf>,
    session: Option<&crate::remote::client::Session>,
) -> Result<()> {
    ensure!(
        !paths.is_empty(),
        "provide file paths as arguments or one per stdin line"
    );
    let started = std::time::Instant::now();
    tracing::info!(
        files = paths.len(),
        parallelism = options.parallelism.get(),
        remote = session.is_some(),
        "Starting source identify command"
    );
    tracing::debug!(?options, "Selected source identify options");
    let runtime = runtime::Runtime::new()?;
    let progress = crate::commands::mod_support::progress::Progress::new();
    progress.set_total(paths.len());
    let columns = options.columns.selected();
    let source = SourceId(
        options
            .source
            .split('@')
            .next()
            .unwrap_or(&options.source)
            .to_owned(),
    );
    let mut output = io::stdout().lock();
    let mut buffered = vec![None; paths.len()];
    let mut next = 0;
    let mut failed = 0;
    let mut remote_failure = false;
    let mut emit = |event| -> Result<()> {
        if let Event::Done { failed: total } = &event {
            ensure!(
                *total == failed && next == paths.len(),
                "identification completion disagrees with ordered results"
            );
            tracing::debug!(
                failed = total,
                rows = next,
                "Finished ordered identification output"
            );
        }
        if let Event::Item {
            indices, outcome, ..
        } = event
        {
            for index in indices {
                buffered[index] = Some(outcome.clone());
                progress.advance();
            }
            // A fast later lookup advances progress but never overtakes an earlier row/error.
            while next < buffered.len() {
                let Some(outcome) = buffered[next].take() else {
                    break;
                };
                match outcome {
                    Outcome::Matched { file } => output::write_match(
                        &mut output,
                        next + 1,
                        &paths[next],
                        &source,
                        Some(&file),
                        &columns,
                    )?,
                    Outcome::Unmatched => output::write_match(
                        &mut output,
                        next + 1,
                        &paths[next],
                        &source,
                        None,
                        &columns,
                    )?,
                    Outcome::Failed { error, remote } => {
                        let error = error.into_error();
                        let error = if remote {
                            crate::diagnostics::RemoteError::wrap(error)
                        } else {
                            error
                        };
                        progress.report(&error);
                        failed += 1;
                        remote_failure |= remote;
                    }
                }
                next += 1;
            }
            output.flush()?;
        }
        Ok(())
    };
    let result = runtime.block_on(async {
        let location = session.map(Location::Remote).unwrap_or(Location::Local);
        Identification::start(
            paths.clone(),
            &[options.source],
            options.parallelism.get(),
            location,
        )?
        .run(&mut emit)
        .await
    });
    drop(progress);
    tracing::info!(
        files = paths.len(),
        failed,
        success = result.is_ok() && failed == 0,
        elapsed_ms = started.elapsed().as_millis(),
        "Source identify command ended"
    );
    result?;
    if failed > 0 {
        let error = anyhow::anyhow!("failed to identify {failed} file(s)");
        return Err(if remote_failure {
            crate::diagnostics::RemoteError::wrap(error)
        } else {
            error
        });
    }
    Ok(())
}
