//! Display initialization diagnostics and results.

use std::io::{self, Write};

use mcpm_backend::operations::server::init::InitRconOutcome;
use mcpm_lib::local::mc::server::identify::SkippedStatement;
use mcpm_rich::rich;

use crate::{
    display::{
        console::ConsoleWriter,
        rich::{block, line, tip},
        style::{OutputStyle, text},
    },
    terminal,
};

pub(super) fn write_skipped_statement(
    output: &mut impl Write,
    warning: &SkippedStatement,
) -> io::Result<()> {
    let style = OutputStyle::stderr();
    let path = text(&warning.script.display().to_string());
    let reason = text(&warning.reason);
    let line = warning.line;
    block(
        &mut ConsoleWriter::new(output),
        &rich!("[warning:](warning) Skipping `{path}:{line}`: {reason}"),
        &style,
    )
}

/// Import diagnostics stay on stderr, including Raw mode, leaving server records unchanged.
pub(super) fn write_publication(
    output: &mut impl Write,
    publication: &mcpm_backend::operations::server::init::Publication,
) -> io::Result<()> {
    let style = OutputStyle::stderr();
    let mut output = ConsoleWriter::new(output);
    for (action, paths) in [
        ("Created", &publication.created),
        ("Replaced", &publication.replaced),
        ("Preserved", &publication.preserved),
        ("Removed", &publication.removed),
    ] {
        for path in paths {
            writeln!(
                output,
                "{} {}",
                style.muted(action),
                style.muted(text(&path.display().to_string()))
            )?;
        }
    }
    if !publication.imported.is_empty() && !publication.preserved.is_empty() {
        let warning = rich!(
            "[Existing](warning) `conf.toml` [was preserved; source tuning settings were not
             merged.](warning)"
        );
        block(&mut output, &warning, &style)?;
    }
    Ok(())
}

pub(super) fn write_success(output: &mut impl Write, launch_missing: bool) -> io::Result<()> {
    if !terminal::stdout_is_tty() {
        return Ok(());
    }
    let mut output = ConsoleWriter::new(output);
    writeln!(output)?;
    let style = OutputStyle::new();
    block(
        &mut output,
        &rich!("[Initialized successfully.](success)"),
        &style,
    )?;
    if launch_missing {
        let server = &crate::program::roots().server;
        tip(
            &mut output,
            &rich!(
                "
                 `launch.toml` is missing. Before running `{server} launch`:

                 - Import a startup script: run `{server} init --force --start-script run.sh`
                   (use `run.bat` on Windows).
                 - Configure it manually: rerun `{server} init --force` with the server identity,
                   `--entry`, and the original JVM and application arguments.

                 `--force` preserves `conf.toml` by default."
            ),
            &style,
        )?;
    }
    Ok(())
}

pub(super) fn write_rcon_warning(output: &mut impl Write, message: &str) -> io::Result<()> {
    let style = OutputStyle::stderr();
    let escaped = text(message);
    block(
        &mut ConsoleWriter::new(output),
        &rich!("[warning:](warning) {escaped}"),
        &style,
    )
}

pub(super) fn write_rcon_outcome(
    output: &mut impl Write,
    outcome: &InitRconOutcome,
) -> io::Result<()> {
    let mut writer = ConsoleWriter::new(output);
    let style = OutputStyle::stderr();
    match outcome {
        InitRconOutcome::Enabled {
            port,
            generated_password,
        } => {
            block(
                &mut writer,
                &rich!("[Successfully configured RCON.](success)"),
                &style,
            )?;
            writeln!(
                writer,
                "{}",
                line(&rich!("RCON port: [{port}](secondary)"), &style)
            )?;
            if let Some(password) = generated_password {
                let message = if terminal::stderr_is_tty() {
                    let escaped = text(password);
                    rich!("RCON password: [{escaped}](secondary)")
                } else {
                    rich!("Generated RCON password was written to `server.properties`.")
                };
                writeln!(writer, "{}", line(&message, &style))?;
            }
        }
        InitRconOutcome::Disabled => {
            block(
                &mut writer,
                &rich!(
                    "[Disabled incomplete RCON configuration in](muted) `server.properties`[.](muted)"
                ),
                &style,
            )?;
        }
        InitRconOutcome::Warning { message } => {
            write_rcon_warning(output, message)?;
        }
    }
    Ok(())
}
