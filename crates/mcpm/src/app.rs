//! Shared process startup, terminal policy, initialization and command execution.

use std::{process::ExitCode, time::SystemTime};

use clap::ColorChoice;

use crate::{
    build_info, cli, commands, diagnostics, display, initcall, logging,
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
        if build_info::is_unofficial_release() {
            let mut stderr = std::io::stderr().lock();
            let _ = display::build_info::unofficial_warning(
                &mut display::console::ConsoleWriter::new(&mut stderr),
            );
        }
        initcall::run()?;
        commands::run(cli.command, cli.ssh)
    })();
    match result {
        Ok(commands::CommandOutcome::Completed) => ExitCode::SUCCESS,
        Ok(commands::CommandOutcome::Exited(code)) => {
            #[cfg(windows)]
            std::process::exit(code as i32);
            #[cfg(not(windows))]
            {
                ExitCode::from(code as u8)
            }
        }
        Err(error) => {
            diagnostics::error(&error);
            ExitCode::FAILURE
        }
    }
}
