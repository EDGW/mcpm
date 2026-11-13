//! Recursive long-help export from the current executable's native Clap command tree.

use std::io::{self, Write};

use anyhow::{Context, Result};
use clap::{ColorChoice, Command};
use mcpm_rich::rich;

use crate::{
    cli,
    display::{rich::line, style::OutputStyle},
    program::Program,
    terminal,
};

pub(super) fn run(program: Program) -> Result<()> {
    let colored = terminal::stdout_color_enabled();
    let mut command = cli::command(
        program,
        if colored {
            ColorChoice::Always
        } else {
            ColorChoice::Never
        },
    );
    // Build the complete tree once so inherited options, deferred schemas and full
    // invocation paths match the corresponding ordinary --help commands.
    command.build();
    let mut path = program.name().to_owned();
    let mut output = io::stdout().lock();
    write_help(
        &mut command,
        &mut path,
        &mut output,
        &OutputStyle::new(),
        colored,
    )
    .context("failed to write recursive command help")?;
    output
        .flush()
        .context("failed to flush recursive command help")
}

fn write_help(
    command: &mut Command,
    path: &mut String,
    output: &mut impl Write,
    style: &OutputStyle,
    colored: bool,
) -> io::Result<()> {
    writeln!(
        output,
        "{}\n",
        line(&rich!("[=== {path} ===](primary)"), style)
    )?;
    let help = command.render_long_help();
    if colored {
        write!(output, "{}", help.ansi())?;
    } else {
        write!(output, "{help}")?;
    }
    writeln!(output)?;
    for child in command.get_subcommands_mut() {
        // Clap's generated `help` route repeats this tree; visit canonical
        // application commands instead of recursing through that synthetic copy.
        if child.get_name() == "help" {
            continue;
        }
        let previous = path.len();
        path.push(' ');
        path.push_str(child.get_name());
        write_help(child, path, output, style, colored)?;
        path.truncate(previous);
    }
    Ok(())
}
