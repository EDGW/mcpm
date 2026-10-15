mod cli;
mod commands;
mod diagnostics;
mod logging;
mod terminal;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    terminal::init(cli.raw);
    if let Err(error) = logging::init(cli.log_level) {
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
