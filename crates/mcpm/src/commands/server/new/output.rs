//! Console and Raw results for vanilla creation; consent warnings remain on stderr.

use crate::{
    display::{
        console::ConsoleWriter,
        rich::{block, line},
        style::OutputStyle,
    },
    terminal,
};
use anyhow::Result;
use mcpm_backend::operations::server::new::{Installed, RconStatus};
use mcpm_rich::rich;
use std::io::{self, Write};

pub(super) fn write(info: &Installed, full_command: bool, accepted: bool) -> Result<()> {
    let mut stdout = io::stdout().lock();
    super::super::git_output::write_git(&mut stdout, info.git.as_ref())?;
    crate::display::server::write_snapshot(&mut stdout, &info.server, full_command, None)?;
    if terminal::stdout_is_tty() {
        let mut output = ConsoleWriter::new(&mut stdout);
        writeln!(output)?;
        block(
            &mut output,
            &rich!("[Server created successfully.](success)"),
            &OutputStyle::new(),
        )?;
    }
    stdout.flush()?;
    {
        let mut stderr = io::stderr().lock();
        let mut output = ConsoleWriter::new(&mut stderr);
        let style = OutputStyle::stderr();
        match &info.rcon {
            RconStatus::Created { port, password } => {
                block(
                    &mut output,
                    &rich!("[Successfully configured RCON.](success)"),
                    &style,
                )?;
                writeln!(
                    output,
                    "{}",
                    line(&rich!("RCON port: [{port}](secondary)"), &style)
                )?;
                writeln!(
                    output,
                    "{}",
                    line(&rich!("RCON password: [{password}](secondary)"), &style)
                )?;
            }
            RconStatus::UnsupportedVersion => {
                let version = &info.server.minecraft_version;
                block(
                    &mut output,
                    &rich!(
                        "[Minecraft {version} was released before 1.9-pre4; RCON server.properties was not generated.](muted)"
                    ),
                    &style,
                )?;
            }
            RconStatus::Disabled => {}
        }
    }
    if !accepted {
        let url = mcpm_lib::download::eula::EULA_URL;
        block(
            &mut ConsoleWriter::new(&mut io::stderr().lock()),
            &rich!(
                "[Minecraft EULA has not been accepted.](warning) Review `{url}` and edit the server's `eula.txt` before launching."
            ),
            &OutputStyle::stderr(),
        )?;
    }
    Ok(())
}
