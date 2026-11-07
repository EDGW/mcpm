//! Source-identification CLI options and ordered presentation of the Host result stream.

use crate::commands::supports::mods::reuse::QueryArgs;
use mcpm_lib::local::cache::mods::ModsCacheSelection;
use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Result, ensure};
use clap::Args;
use mcpm_backend::operations::mods::source::{
    Configuration,
    identify::{Event, Identify, Input, Target},
};
use std::num::NonZeroUsize;
use std::sync::Arc;

use crate::commands::execution::{DEFAULT_PARALLELISM, Runtime};

mod columns;
mod model;
mod output;
pub(in crate::commands) use model::Location;

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: Console (field groups) and Raw (TSV; --raw or redirected stdout); Remote supported through --ssh.\n\nPaths are literal file paths on the selected execution host; file reads, cache reuse and source requests run there. Redirected stdin is appended, one path per line.\n--columns, --column and -C select fields; --headers adds a Raw header. Optional Raw columns query_status, cache_coverage and origin describe Host query results.\nThe default cache is read-only: hits skip source requests, misses never write. --no-cache disables lookup; --cache-dir selects an initialized cache on the file host.\nUnmatched files are reported as unmatched; preparation or source failures exit unsuccessfully while preserving available matches.\n\nExamples:\n  {tools} mods source identify --source modrinth ./mods/create.jar\n  {tools} mods source identify --source curseforge ./mods/a.jar ./mods/b.jar\n  {tools} mods source identify --source 'curseforge@KEY' ./mods/create.jar\n  printf '%s\\n' ./mods/a.jar ./mods/b.jar | {tools} mods source identify --source modrinth\n  {tools} --raw mods source identify --source modrinth ./mods/create.jar --columns path,project,id,status,query_status,cache_coverage,origin", tools = crate::program::roots().tools)
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
    source: Configuration,
    /// Maximum simultaneous file lookups; results retain input order.
    #[arg(short = 'p', long, default_value_t = NonZeroUsize::new(DEFAULT_PARALLELISM).unwrap(), value_name = "COUNT")]
    parallelism: NonZeroUsize,
    #[command(flatten)]
    columns: columns::ColumnArgs,
    /// Include one header row in Raw TSV output; ignored by Console.
    #[arg(long)]
    headers: bool,
    #[command(flatten)]
    pub cache: QueryArgs,
}

impl IdentifyOptions {
    pub(in crate::commands) fn parallelism(&self) -> usize {
        self.parallelism.get()
    }
}

impl std::fmt::Debug for IdentifyOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentifyOptions")
            .field("source", &self.source.label())
            .field("parallelism", &self.parallelism)
            .field("columns", &self.columns)
            .finish()
    }
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands) fn run(args: IdentifyArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let paths = crate::commands::stdin::append(args.paths)?;
    ensure!(
        !paths.is_empty(),
        "provide file paths as arguments or one per stdin line"
    );
    let cache = if args.options.cache.no_cache {
        None
    } else {
        Some(
            args.cache_dir
                .map(|path| ModsCacheSelection::Directory { path })
                .unwrap_or(ModsCacheSelection::Default),
        )
    };
    let runtime = Runtime::new()?;
    let (executor, _, _) = runtime.block_on(ssh.create_executor(None))?;
    present(
        args.options,
        paths,
        Location::new(&executor, cache.as_ref()),
    )
}

pub(in crate::commands) fn run_selection_at(
    options: IdentifyOptions,
    scope: mcpm_backend::operations::mods::loading::ScopeInput,
    selection: mcpm_backend::operations::mods::loading::selection::SelectionInput,
    executor: &mcpm_exec::service::Executor,
) -> Result<()> {
    present_target(
        options,
        Target::Selection { scope, selection },
        Location::new(executor, None),
    )
}

fn present(options: IdentifyOptions, paths: Vec<PathBuf>, location: Location<'_>) -> Result<()> {
    ensure!(
        !paths.is_empty(),
        "provide file paths as arguments or one per stdin line"
    );
    present_target(
        options,
        Target::Paths {
            paths,
            cache: location.cache.cloned(),
        },
        location,
    )
}

fn present_target(options: IdentifyOptions, target: Target, location: Location<'_>) -> Result<()> {
    let runtime = Runtime::new()?;
    let progress =
        crate::progress::Progress::new("Preparing file identification...", "Identifying files");
    let columns = options.columns.selected();
    let mut output = io::stdout().lock();
    output::headers(&mut output, &columns, options.headers)?;
    let parallelism = options.parallelism();
    runtime.block_on(async {
        let mut stream = location.executor.call::<Identify>(Input {
            target,
            sources: vec![options.source],
            force: false,
            parallelism,
        }).await?;
        let mut paths = Vec::new();
        let mut buffered = Vec::new();
        let mut next = 0;
        let mut summary = None;
        let mut prepared = false;
        while let Some(event) = stream.next().await.transpose()? {
            match event {
                Event::Prepared { paths: resolved, diagnostics, .. } => {
                    ensure!(!prepared, "duplicate identification preparation");
                    prepared = true;
                    paths = resolved;
                    buffered = vec![None; paths.len()];
                    progress.set_total(paths.len());
                    for error in diagnostics {
                        progress.report(error.as_inner());
                    }
                }
                Event::Loading { .. } => {}
                Event::Item { indices, result } => {
                    ensure!(prepared, "missing identification preparation");
                    let result = result.map(Arc::new).map_err(Arc::new);
                    for index in indices {
                        ensure!(index < buffered.len(), "invalid identification result index");
                        if index >= next && buffered[index].is_none() {
                            buffered[index] = Some(result.clone());
                            progress.advance();
                        }
                    }
                    // Duplicate paths retain their input rows; fast later results never overtake them.
                    while next < buffered.len() {
                        let Some(result) = buffered[next].take() else { break };
                        match result {
                            Ok(result) => {
                                output::write_result(&mut output, next + 1, &paths[next], &result, &columns)?;
                                for error in result.errors() {
                                    progress.report(error.as_inner());
                                }
                            }
                            Err(error) => progress.report(error.as_inner()),
                        }
                        next += 1;
                    }
                    output.flush()?;
                }
                Event::Done(done) => summary = Some(done),
            }
        }
        ensure!(prepared, "missing identification preparation");
        ensure!(next == paths.len(), "missing identification file results");
        let summary = summary.ok_or_else(|| anyhow::anyhow!("missing identification completion"))?;
        ensure!(
            !summary.has_failures(),
            "identification failed for {} file(s), partially failed for {} file(s), with {} preparation failure(s) and {} selection failure(s)",
            summary.failed,
            summary.partial_failed,
            summary.preparation_failed,
            summary.selection_failed,
        );
        Ok(())
    })
}
