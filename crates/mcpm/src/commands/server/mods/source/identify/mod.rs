//! Resolves server-owned selections through `mods.select`, then identifies the paths.

use anyhow::{Result, ensure};
use clap::Args;
use mcpm_backend::data::runtime::RuntimeEnvironment;
use mcpm_backend::mods::types::{ModsCachePolicy, ModsScope};
use mcpm_backend::mods::{Select, SelectInput};
use mcpm_backend::mods::selection::SelectionSpec;

use super::super::input::FileArgs;
use crate::commands::{
    execution::failure as operation_failure,
    server::target,
    source_support::identify,
    supports::mods::{inspect, scan_output::ScanOutput},
};
use crate::ssh::SshArgs;

#[derive(Args, Debug)]
#[command(
    after_help = format!("Modes: Console (field groups) and Raw (TSV; --raw or redirected stdout); Remote supported through --ssh.\n\nWith --ssh, server paths and file reads are remote; source requests and credentials remain local.\nInputs match filename, then exact modid, then exact mod name in the server's mods directory.\nPaths are relative to that directory; mods/ prefixes are accepted. Use --all to select every mod.\nRedirected stdin appends one selection per line; --input sets its interpretation.\nMatching uses file contents and is independent of Minecraft version or loader compatibility.\n--columns, --column and -C select fields; --headers adds a Raw header.\n\nExamples:\n  {server} mods source identify --server ./my-server --source modrinth --all\n  {server} mods source identify --server ./my-server --source modrinth create\n  {server} --ssh 'ssh host' mods source identify --server /srv/mc --source curseforge --all\n  printf '%s\\n' create sodium | {server} mods source identify --source modrinth --all", server = crate::program::roots().server)
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
    let target = target::resolve(server, ssh)?;
    let remote = target.session.target().is_remote();
    let parallelism = args.options.parallelism();
    let request = SelectInput {
        scope: ModsScope::Server {
            path: target.path.clone(),
        },
        selection: SelectionSpec::from_selection(selection)?,
        parallelism,
        cache: if args.options.cache.no_cache {
            ModsCachePolicy::Disabled
        } else {
            ModsCachePolicy::ScopeDefault
        },
        hide_unrecognized: true,
    };
    let runtime = crate::commands::execution::Runtime::new()?;
    let summary = runtime.block_on(async {
        let mut handle = target
            .session
            .start::<Select>(request)
            .await
            .map_err(|failure| operation_failure(failure, remote))?;
        let output = ScanOutput::default();
        while let Some(event) = handle.next().await {
            match event {
                mcpm_backend::mods::records::inspect::Event::Scan { completed, total } => {
                    output.event(crate::commands::supports::mods::scan::Event::Progress {
                        completed,
                        total,
                    })?;
                }
                mcpm_backend::mods::records::inspect::Event::Error { error } => {
                    crate::logging::record_error(&error.clone().into_error());
                    output.event(crate::commands::supports::mods::scan::Event::Diagnostic {
                        error,
                    })?;
                }
                _ => {}
            }
        }
        handle
            .finish()
            .await
            .map_err(|failure| operation_failure(failure, remote))
    })?;
    if summary.failed > 0 {
        let error = anyhow::anyhow!(
            "failed to resolve {} server mod selection(s)",
            summary.failed
        );
        return Err(if remote {
            crate::diagnostics::RemoteError::wrap(error)
        } else {
            error
        });
    }
    if summary.paths.is_empty() {
        return Ok(());
    }
    let environment = RuntimeEnvironment::new().with_mods_cache(
        (!args.options.cache.no_cache).then_some(mcpm_backend::data::cache::mods::ModsCacheSelection::Default),
    );
    let location = if remote {
        identify::Location::Remote {
            session: target.session.clone(),
            environment: environment.clone(),
        }
    } else {
        identify::Location::Local {
            session: target.session.clone(),
            environment: environment.clone(),
        }
    };
    identify::run_paths_at(args.options, summary.paths, location)?;
    crate::commands::execution::finish(&runtime, &target.session)?;
    ensure!(
        summary.failed == 0,
        "failed to resolve {} server mod selection(s)",
        summary.failed
    );
    Ok(())
}
