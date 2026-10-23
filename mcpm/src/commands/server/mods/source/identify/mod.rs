//! Resolves server-owned selections before local provider identification of file summaries.

use anyhow::{Result, ensure};
use clap::Args;

use super::super::input::FileArgs;
use crate::{
    commands::{
        mod_support::{inspect, scan_output::ScanOutput, selection},
        server::{
            mod_source::ServerMods,
            target::{self, Target},
        },
        source_support::identify,
    },
    ssh::SshArgs,
};

mod remote;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout); Remote supported through --ssh.\n\nWith --ssh, server paths and file reads are remote; source requests and credentials remain local.\nInputs match filename, then exact modid, then exact mod name in the server's mods directory.\nPaths are relative to that directory; mods/ prefixes are accepted. Use --all to select every mod.\nRedirected stdin appends one selection per line; --input sets its interpretation.\nMatching uses file contents and is independent of Minecraft version or loader compatibility.\n\nExamples:\n  mcpm server mods source identify --server ./my-server --source modrinth --all\n  mcpm server mods source identify create.jar --source curseforge\n  mcpm server mods source identify --modid create --source modrinth --columns path,project,id,status\n  printf '%s\\n' create | mcpm server mods source identify --source modrinth --input modid"
)]
pub(super) struct IdentifyArgs {
    #[command(flatten)]
    files: FileArgs,
    #[command(flatten)]
    options: identify::IdentifyOptions,
}

pub(super) fn run(args: IdentifyArgs, ssh: SshArgs) -> Result<()> {
    let (server, selection) = args.files.read()?;
    if selection.is_empty() {
        inspect::prompt_for_input()?;
        return Ok(());
    }
    let Target {
        path: server, ssh, ..
    } = target::resolve(server, ssh)?;
    let parallelism = args.options.parallelism();
    let (paths, failed, mut session) = if ssh.is_requested() {
        tracing::info!("Opening SSH data source for server source identification");
        let session = ssh.connect()?;
        let (paths, failed) = remote::resolve(&session, server, selection, parallelism)?;
        (paths, failed, Some(session))
    } else {
        let server = ServerMods::load(server)?;
        let output = ScanOutput::default();
        let (paths, failed) = selection::resolve_paths(
            selection,
            server.scope(),
            server.loaders(),
            parallelism,
            || Ok(()),
            |event| output.event(event),
        )?;
        (paths, failed, None)
    };
    let result = if paths.is_empty() {
        Ok(())
    } else {
        let location = session
            .as_ref()
            .map(identify::Location::Remote)
            .unwrap_or(identify::Location::Local);
        identify::run_paths_at(args.options, paths, location)
    };
    if let Some(session) = session.as_mut() {
        session.close()?;
    }
    result?;
    let result = (|| {
        ensure!(
            failed == 0,
            "failed to resolve {failed} server mod selection(s)"
        );
        Ok(())
    })();
    result.map_err(|error| {
        if session.is_some() {
            crate::diagnostics::RemoteError::wrap(error)
        } else {
            error
        }
    })
}
