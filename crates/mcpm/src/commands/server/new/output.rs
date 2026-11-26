//! Console and Raw results for server creation; consent warnings and loader errors remain on stderr.

use std::io::{self, Write};

use anyhow::Result;
use mcpm_backend::operations::server::{
    loader::LoaderRequest,
    new::{Installed, RconStatus},
};
use mcpm_exec::error::PortableError;
use mcpm_rich::rich;

use crate::{
    display::{
        console::ConsoleWriter,
        rich::{block, line},
        style::OutputStyle,
    },
    server::RemoteInfo,
    terminal,
};

pub(super) fn write(
    info: &Installed,
    full_command: bool,
    accepted: bool,
    remote: Option<&RemoteInfo>,
    loader_error: Option<&PortableError>,
    requested_loader: Option<&LoaderRequest>,
) -> Result<()> {
    let mut stdout = io::stdout().lock();
    super::super::git_output::write_git(&mut stdout, info.git.as_ref())?;
    crate::display::server::write_snapshot(&mut stdout, &info.server, full_command, remote)?;
    if terminal::stdout_is_tty() {
        let mut output = ConsoleWriter::new(&mut stdout);
        writeln!(output)?;
        if loader_error.is_some() {
            block(
                &mut output,
                &rich!("[Vanilla server created, but mod loader installation failed.](warning)"),
                &OutputStyle::new(),
            )?;
        } else {
            block(
                &mut output,
                &rich!("[Server created successfully.](success)"),
                &OutputStyle::new(),
            )?;
        }
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
    if let Some(error) = loader_error {
        crate::diagnostics::error(error.as_inner());
        if let Some(loader) = requested_loader {
            let target = match &loader.version {
                Some(version) => format!("{}:{version}", loader.family),
                None => loader.family.to_string(),
            };
            let retry = crate::program::roots()
                .server
                .command(&format!("add-loader {target}"));
            block(
                &mut ConsoleWriter::new(&mut io::stderr().lock()),
                &rich!(
                    "The Vanilla server is intact. Retry loader installation with `{retry}`."
                ),
                &OutputStyle::stderr(),
            )?;
        }
    }
    Ok(())
}
