//! CLI entry point and initialization of terminal policy, theme and logging.

mod cli;
mod commands;
mod diagnostics;
mod help;
mod initcall;
mod logging;
mod progress;
mod remote;
mod ssh;
mod terminal;
mod theme;
mod widgets;

use std::{process::ExitCode, time::SystemTime};

use clap::{ColorChoice, CommandFactory, FromArgMatches};

fn main() -> ExitCode {
    let started = SystemTime::now();
    let args: Vec<_> = std::env::args_os().collect();
    let raw = args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--raw")
        || args.get(1).is_some_and(|arg| arg == "remote");
    let mut command =
        cli::Cli::command().color(if !raw && std::env::var_os("NO_COLOR").is_none() {
            ColorChoice::Auto
        } else {
            ColorChoice::Never
        });
    let matches = command
        .try_get_matches_from_mut(args)
        .unwrap_or_else(|error| error.exit());
    let cli = cli::Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    terminal::init(raw || matches!(cli.command, cli::Command::Remote(_)));
    if let Err(error) = logging::init(
        cli.log_level,
        started,
        matches!(cli.command, cli::Command::Remote(_)),
        cli.remote_logs,
    ) {
        diagnostics::error(&error);
        return ExitCode::FAILURE;
    }
    if let Err(error) = theme::init() {
        diagnostics::error(&error);
        return ExitCode::FAILURE;
    }
    if let Err(error) = initcall::run() {
        diagnostics::error(&error);
        return ExitCode::FAILURE;
    }
    match commands::run(cli.command, cli.ssh) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            diagnostics::error(&error);
            ExitCode::FAILURE
        }
    }
}
