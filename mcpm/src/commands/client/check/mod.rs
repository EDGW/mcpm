//! Client resource check command selection, execution and presentation.

mod options;
mod output;
mod progress;

use std::{
    collections::HashSet,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::local::mc::client::MinecraftDirectory;
use thiserror::Error;

use super::{input, output as client_output, platform, selector};

#[derive(Args, Debug)]
pub(super) struct CheckArgs {
    /// Version directory IDs; omit to select one with arrow keys.
    #[arg(value_name = "VERSION")]
    versions: Vec<String>,
    /// Minecraft root; defaults to the last .minecraft in the current path, or the current directory.
    #[arg(short = 'd', long, value_name = "PATH")]
    directory: Option<PathBuf>,
    /// Check every version, including broken profiles, without prompting.
    #[arg(long, conflicts_with = "versions")]
    all: bool,
    #[command(flatten)]
    checks: options::OptionArgs,
}

#[derive(Debug, Error)]
enum CheckError {
    #[error("{versions} client version(s) could not be fully verified")]
    Incomplete { versions: usize },
}

pub(super) fn run(args: CheckArgs) -> Result<()> {
    let input = input::resolve(args.directory)?;
    let directory = MinecraftDirectory::new(input.path)?;
    if input.inferred {
        client_output::write_inferred_root(
            &mut io::stderr().lock(),
            directory.path(),
            &crate::program::roots()
                .client
                .command("check --directory ."),
        )
        .context("failed to write Minecraft root hint")?;
    }
    let mut load_errors = 0;
    let versions = if args.all {
        directory.versions()?
    } else if args.versions.is_empty() {
        let versions = directory.versions()?;
        let Some(selected) = selector::select(&versions, selector::Purpose::Check)? else {
            return Ok(());
        };
        versions.into_iter().nth(selected).into_iter().collect()
    } else {
        let mut seen = HashSet::new();
        let mut versions = Vec::new();
        for name in args.versions {
            if !seen.insert(name.clone()) {
                continue;
            }
            match directory
                .version(&name)
                .with_context(|| format!("failed to load client version {name:?}"))
            {
                Ok(version) => versions.push(version),
                Err(error) => {
                    crate::diagnostics::error(&error);
                    load_errors += 1;
                }
            }
        }
        versions
    };
    if versions.is_empty() {
        if load_errors > 0 {
            return Err(CheckError::Incomplete {
                versions: load_errors,
            }
            .into());
        }
        return Ok(());
    }
    let options = args.checks.selected();
    let rules = platform::current().context("failed to determine client check platform")?;
    let mut failed = load_errors;
    let mut stdout = io::stdout().lock();
    let batch = progress::BatchProgress::new(versions.len());
    for version in &versions {
        let mut progress = batch.start(version.name(), directory.path());
        let report =
            directory.check_with_progress(version, options, &rules, |event| progress.update(event));
        drop(progress);
        batch.finish_version();
        failed += usize::from(!report.is_intact());
        batch.suspend(|| -> Result<()> {
            for issue in &report.issues {
                let error = crate::diagnostics::ErrorReport::capture_std(&issue.problem)
                    .into_error()
                    .context(format!(
                        "client profile {:?}: {}",
                        report.version,
                        output::target_name(&issue.target)
                    ));
                let error = match &issue.path {
                    Some(path) => error.context(format!("resource {path:?}")),
                    None => error,
                };
                crate::diagnostics::error(&error);
            }
            output::write_report(&mut stdout, &report)
                .context("failed to write client check report")?;
            stdout
                .flush()
                .context("failed to flush client check report")
        })?;
    }
    if failed > 0 {
        return Err(CheckError::Incomplete { versions: failed }.into());
    }
    Ok(())
}
