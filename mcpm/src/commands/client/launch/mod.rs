mod options;
mod output;

use std::{collections::HashSet, io, path::PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use kako_mods_lib::local::mc::client::{MinecraftDirectory, launch::LaunchProcess};
use thiserror::Error;

use super::{input, output as client_output, platform, selector};

#[derive(Args)]
pub(super) struct LaunchArgs {
    /// Version directory IDs; omit to select one with Up/Down and Enter.
    #[arg(value_name = "VERSION")]
    versions: Vec<String>,
    /// Minecraft root; defaults to the last .minecraft in pwd or pwd itself.
    #[arg(short = 'd', long, value_name = "PATH")]
    directory: Option<PathBuf>,
    #[command(flatten)]
    options: options::OptionArgs,
}

#[derive(Debug, Error)]
enum LaunchError {
    #[error("{versions} client version(s) failed to launch or exited unsuccessfully")]
    Incomplete { versions: usize },
}

pub(super) fn run(args: LaunchArgs) -> Result<()> {
    let input = input::resolve(args.directory)?;
    let directory = MinecraftDirectory::new(input.path)?;
    if input.inferred {
        client_output::write_inferred_root(
            &mut io::stderr().lock(),
            directory.path(),
            "mcpm client launch --directory .",
        )
        .context("failed to write Minecraft root hint")?;
    }
    let names = if args.versions.is_empty() {
        let versions = directory.versions()?;
        let Some(selected) = selector::select(&versions, selector::Purpose::Launch)? else {
            return Ok(());
        };
        vec![versions[selected].name().to_owned()]
    } else {
        args.versions
    };
    let rules = platform::current().context("failed to determine client launch platform")?;
    let mut seen = HashSet::new();
    let mut failed = 0;
    let mut reporter = output::Reporter::new();
    let mut processes: Vec<(String, LaunchProcess)> = Vec::new();
    for name in names {
        if !seen.insert(name.clone()) {
            continue;
        }
        reporter.event(&name, output::Event::Preparing);
        let start = || -> Result<LaunchProcess> {
            let version = directory.version(&name)?;
            let options = args.options.build(&version, &rules)?;
            let plan = directory.plan_launch(&version, &options)?;
            Ok(plan.prepare()?.spawn()?)
        };
        match start()
            .with_context(|| format!("failed to launch client {:?}", client_output::text(&name)))
        {
            Ok(process) => {
                reporter.event(&name, output::Event::Started { pid: process.id() });
                processes.push((name, process));
            }
            Err(error) => {
                crate::diagnostics::error(&error);
                failed += 1;
            }
        }
    }
    // All selected versions get a launch attempt before waiting for any game.
    for (name, mut process) in processes {
        match process
            .wait()
            .with_context(|| format!("failed to wait for client {:?}", client_output::text(&name)))
        {
            Ok(status) => {
                reporter.event(&name, output::Event::Exited { status });
                failed += usize::from(!status.success());
            }
            Err(error) => {
                crate::diagnostics::error(&error);
                failed += 1;
            }
        }
    }
    reporter
        .finish()
        .context("failed to write client launch status")?;
    if failed > 0 {
        return Err(LaunchError::Incomplete { versions: failed }.into());
    }
    Ok(())
}
