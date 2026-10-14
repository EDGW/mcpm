mod cli;
mod commands;
mod diagnostics;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    match commands::run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            diagnostics::error(&error);
            ExitCode::FAILURE
        }
    }
}
