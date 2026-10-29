//! Source-identification CLI options and ordered presentation over the shared flow.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Result, ensure};
use clap::Args;
use mcpm_backend::data::cache::mods::ModsCacheSelection;
use mcpm_backend::source::types::SourceId;
use std::num::NonZeroUsize;

use crate::commands::execution::{DEFAULT_PARALLELISM, Runtime};
use crate::flows::identify::{self, Event, Outcome, Target};

mod columns;
mod output;

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: Console (field groups) and Raw (TSV; --raw or redirected stdout); Remote supported through --ssh.\n\nPaths are literal file paths, resolved on the remote host with --ssh. Redirected stdin is appended, one path per line. --columns, --column and -C select fields; --headers adds a Raw header.\nThe default cache is read-only: hits skip provider requests, misses never write. --no-cache disables lookup; --cache-dir selects an initialized cache on the file host.\nUnmatched files are reported as unmatched; read or request failures exit unsuccessfully.\n\nExamples:\n  {tools} mods source identify --source modrinth ./mods/create.jar\n  {tools} mods source identify --source curseforge ./mods/a.jar ./mods/b.jar\n  {tools} mods source identify --source 'curseforge@KEY' ./mods/create.jar\n  printf '%s\\n' ./mods/a.jar ./mods/b.jar | {tools} mods source identify --source modrinth\n  {tools} --raw mods source identify --source modrinth ./mods/create.jar --columns path,project,id,status", tools = crate::program::roots().tools)
)]
pub(in crate::commands) struct IdentifyArgs {
    /// Files to identify (remote paths with --ssh) by their contents, not their names.
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    options: IdentifyOptions,
    /// Initialized cache directory on the file host.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "no_cache",
        help_heading = "Loading options"
    )]
    cache_dir: Option<PathBuf>,
}

/// Source and presentation options, independent of local path selection.
#[derive(Args)]
pub(in crate::commands) struct IdentifyOptions {
    /// modrinth, curseforge (Curse.tools), or curseforge@KEY (official API).
    #[arg(long, value_name = "SOURCE")]
    source: String,
    /// Maximum simultaneous file lookups; results retain input order.
    #[arg(short = 'p', long, default_value_t = NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap(), value_name = "COUNT")]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    columns: columns::ColumnArgs,
    /// Include one header row in Raw TSV output; ignored by Console.
    #[arg(long)]
    headers: bool,
    #[command(flatten)]
    pub cache: crate::commands::supports::mods::reuse::QueryArgs,
}

impl IdentifyOptions {
    pub(in crate::commands) fn parallelism(&self) -> usize {
        self.parallelism.get()
    }
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

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands) fn run(args: IdentifyArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    ssh.validate()?;
    let paths = crate::commands::stdin::append(args.paths)?;
    let cache = if args.options.cache.no_cache {
        None
    } else {
        Some(
            args.cache_dir
                .map(|path| ModsCacheSelection::Directory { path })
                .unwrap_or(ModsCacheSelection::Default),
        )
    };
    if !ssh.is_requested() {
        let session = crate::commands::execution::local_session()?;
        return present(args.options, paths, Target { session, cache });
    }
    ensure!(
        !paths.is_empty(),
        "provide file paths as arguments or one per stdin line"
    );
    let _span = tracing::info_span!("source_identify", source = "ssh").entered();
    let session = crate::commands::execution::session_for(&ssh)?;
    present(args.options, paths, Target { session, cache })
}

/// Identifies paths using resources selected by the caller on their owning host.
pub(in crate::commands) fn run_paths_at(
    options: IdentifyOptions,
    paths: Vec<PathBuf>,
    target: Target,
) -> Result<()> {
    present(options, paths, target)
}

fn present(options: IdentifyOptions, paths: Vec<PathBuf>, target: Target) -> Result<()> {
    ensure!(
        !paths.is_empty(),
        "provide file paths as arguments or one per stdin line"
    );
    let started = std::time::Instant::now();
    let remote = !target.is_local();
    tracing::info!(
        files = paths.len(),
        parallelism = options.parallelism.get(),
        remote,
        "Starting source identify command"
    );
    tracing::debug!(?options, "Selected source identify options");
    let runtime = Runtime::new()?;
    let progress =
        crate::progress::Progress::new("Preparing file identification...", "Identifying files");
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
    output::headers(&mut output, &columns, options.headers)?;
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
            indices,
            outcome,
            remote: item_remote,
            ..
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
                    Outcome::Matched { file, .. } => output::write_match(
                        &mut output,
                        next + 1,
                        &paths[next],
                        &source,
                        Some(file.as_ref()),
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
                    Outcome::Failed { error } => {
                        let error = error.into_error();
                        let error = if item_remote {
                            crate::diagnostics::RemoteError::wrap(error)
                        } else {
                            error
                        };
                        progress.report(&error);
                        failed += 1;
                        remote_failure |= item_remote;
                    }
                }
                next += 1;
            }
            output.flush()?;
        }
        Ok(())
    };
    let result = runtime.block_on(async {
        identify::run(
            target,
            vec![options.source],
            paths.clone(),
            options.parallelism.get(),
            &mut emit,
        )
        .await
    });
    drop(progress);
    tracing::debug!(
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
