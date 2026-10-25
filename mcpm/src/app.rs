//! Shared process startup, terminal policy, initialization and command execution.

use std::{process::ExitCode, time::SystemTime};

use clap::ColorChoice;

use crate::{
    cli, commands, diagnostics, initcall, logging,
    program::{CommandRoots, Program},
    terminal, theme,
};

/// Starts one executable using its explicit identity and the shared command implementation.
pub fn run(program: Program) -> ExitCode {
    let started = SystemTime::now();
    CommandRoots::init(program);
    let args: Vec<_> = std::env::args_os().collect();
    let raw = args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--raw");
    let cli = cli::parse(
        program,
        args,
        if !raw && std::env::var_os("NO_COLOR").is_none() {
            ColorChoice::Auto
        } else {
            ColorChoice::Never
        },
    );
    let remote = matches!(cli.command, commands::Command::Remote(_));
    terminal::init(cli.common.raw || remote);
    let result = (|| {
        logging::init(cli.common.log_level, started, remote, cli.remote_logs)?;
        theme::init()?;
        initcall::run()?;
        commands::run(cli.command, cli.ssh)
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            diagnostics::error(&error);
            ExitCode::FAILURE
        }
    }
}
