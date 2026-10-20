//! CLI entry point and initialization of terminal policy, theme and logging.

mod cli;
mod commands;
mod diagnostics;
mod logging;
mod terminal;
mod theme;

use std::process::ExitCode;

use clap::{ColorChoice, CommandFactory, FromArgMatches};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let raw = args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--raw");
    terminal::init(raw);
    let matches = cli::Cli::command()
        .color(
            if !terminal::is_raw() && std::env::var_os("NO_COLOR").is_none() {
                ColorChoice::Auto
            } else {
                ColorChoice::Never
            },
        )
        .get_matches_from(args);
    let cli = cli::Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    if let Err(error) = logging::init(cli.log_level) {
        diagnostics::error(&error);
        return ExitCode::FAILURE;
    }
    if let Err(error) = theme::init() {
        diagnostics::error(&error);
        return ExitCode::FAILURE;
    }
    match commands::run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            diagnostics::error(&error);
            ExitCode::FAILURE
        }
    }
}
