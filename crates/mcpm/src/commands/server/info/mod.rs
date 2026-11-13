//! Server inspection and explicit metadata upgrades, with user decisions kept on the local console.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Args;

use crate::{commands::execution::Runtime, ssh::SshArgs};
use mcpm_backend::operations::server::{
    info::{Info, InfoInput},
    upgrade::{
        ApplyUpgrade, ApplyUpgradeInput, PrepareUpgrade, PrepareUpgradeInput, UpgradeDecision,
    },
};

mod upgrade_prompt;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console (terminal stdout), Raw (--raw or redirected stdout). Explicit --ssh selects the remote host; otherwise workspace registration selects the server. Legacy metadata cannot be used by server commands: run info --upgrade in Console mode first. Console info reports an upgrade error, then shows legacy metadata and exits unsuccessfully. Raw rejects legacy metadata without output, including with --upgrade. A Vanilla candidate requires local yes/no/Cancel confirmation; Cancel is the default. Version 4 needs no upgrade."
)]
pub(super) struct InfoArgs {
    /// Server directory on the selected host; defaults to its current directory.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Upgrade legacy registration; confirm a detected Vanilla identity in the local console.
    #[arg(long)]
    upgrade: bool,
    /// Show the complete startup command generated from launch.toml and conf.toml.
    #[arg(long)]
    full_command: bool,
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn run(args: InfoArgs, ssh: SshArgs) -> Result<crate::commands::CommandOutcome> {
    let runtime = Runtime::new()?;
    let result = runtime.block_on(async {
        let (executor, path, remote) = ssh.create_executor(Some(args.path)).await?;
        let path = path.context("server info requires a path")?;
        let info = if args.upgrade && crate::terminal::stdout_is_tty() {
            let preview = executor
                .call::<PrepareUpgrade>(PrepareUpgradeInput { path: path.clone() })
                .await?;
            if preview.from_version == preview.to_version {
                executor
                    .call::<Info>(InfoInput {
                        path,
                        full_command: args.full_command,
                        allow_legacy: false,
                    })
                    .await?
            } else {
                let decision = if let Some(candidate) = &preview.vanilla {
                    let Some(decision) = upgrade_prompt::choose(candidate)? else {
                        upgrade_prompt::cancelled()?;
                        return Ok::<_, anyhow::Error>(None);
                    };
                    decision
                } else {
                    UpgradeDecision::Preserve
                };
                let info = executor
                    .call::<ApplyUpgrade>(ApplyUpgradeInput {
                        path: path.clone(),
                        expected_revision: preview.revision,
                        decision,
                    })
                    .await?;
                upgrade_prompt::upgraded(preview.from_version, preview.to_version)?;
                if args.full_command {
                    executor
                        .call::<Info>(InfoInput {
                            path,
                            full_command: true,
                            allow_legacy: false,
                        })
                        .await?
                } else {
                    info
                }
            }
        } else {
            executor
                .call::<Info>(InfoInput {
                    path,
                    full_command: args.full_command,
                    allow_legacy: crate::terminal::stdout_is_tty(),
                })
                .await?
        };
        Ok(Some((info, remote)))
    })?;
    let Some((mut info, remote)) = result else {
        return Ok(crate::commands::CommandOutcome::Completed);
    };
    let upgrade_error = info.upgrade_required.take();
    let upgrade_required = upgrade_error.is_some();
    if let Some(report) = upgrade_error {
        crate::diagnostics::error(&report.into_error());
    }
    let mut stdout = io::stdout().lock();
    crate::display::server::write_snapshot(
        &mut stdout,
        &info,
        args.full_command,
        remote.as_ref(),
        true,
    )
    .context("failed to write server information")?;
    stdout
        .flush()
        .context("failed to flush server information")?;
    Ok(if upgrade_required {
        crate::commands::CommandOutcome::Exited(1)
    } else {
        crate::commands::CommandOutcome::Completed
    })
}
