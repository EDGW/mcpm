//! Commands for inspecting Minecraft crash report files.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::Result;
use clap::{Args, Subcommand};
use mcpm_backend::operations::crashreport::{Analyse, AnalyseInput};

use crate::{commands::execution::Runtime, display::crash_report, ssh::SshArgs};

#[derive(Args, Debug)]
pub(crate) struct CrashReportArgs {
    #[command(subcommand)]
    command: CrashReportCommand,
}

#[derive(Subcommand, Debug)]
enum CrashReportCommand {
    /// Inspect a Minecraft crash report file (Console, Raw).
    Info(InfoArgs),
}

#[derive(Args, Debug)]
struct InfoArgs {
    /// Path to the Minecraft crash report file on the selected host.
    #[arg(value_name = "FILE")]
    file: PathBuf,
}

pub(crate) fn run(args: CrashReportArgs, ssh: SshArgs) -> Result<()> {
    match args.command {
        CrashReportCommand::Info(args) => run_info(args, ssh),
    }
}

fn run_info(args: InfoArgs, ssh: SshArgs) -> Result<()> {
    let runtime = Runtime::new()?;
    let analysis = runtime.block_on(async {
        let (executor, _, _) = ssh.create_executor(None).await?;
        executor
            .call::<Analyse>(AnalyseInput {
                path: args.file.clone(),
            })
            .await
    })?;
    let mut stdout = io::stdout().lock();
    crash_report::write_analysis(&mut stdout, &args.file, &analysis)?;
    stdout.flush()?;
    Ok(())
}
