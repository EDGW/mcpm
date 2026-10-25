//! Server-cache request adapter; shared caching owns workflow, reports and validation.

use super::ServerMods;
use crate::commands::supports::mods::reuse::HostEnvironment;
use crate::{
    commands::source_support::SourcesArgs,
    commands::supports::mods::{
        cache::{
            self,
            args::WriteArgs,
            identification::Completion,
            output::{DisplayArgs, Presentation},
        },
        selection::{Selection, SelectionInput},
    },
    remote::command::{Command, Output},
    ssh::SshArgs,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

struct Cache;
impl Command for Cache {
    const ID: &'static str = "server.mods.cache";
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    fn run(contents: Value, output: &Output) -> Result<()> {
        let args: Request =
            serde_json::from_value(contents).context("invalid server mod cache arguments")?;
        let selection = args.selection.into_selection()?;
        ensure!(
            !selection.is_empty(),
            "server mod cache requires mod selections"
        );
        output.checkpoint()?;
        let server = ServerMods::load(args.server)?;
        output.send(HostEnvironment::capture(server.runtime())?)?;
        cache::run(
            selection,
            server.scope(),
            server.runtime(),
            args.writing.options(server.loaders())?,
            || output.checkpoint(),
            |event| {
                match &event {
                    cache::Event::Selection(
                        crate::commands::supports::mods::scan::Event::Diagnostic { error },
                    ) => crate::logging::record_error(&error.clone().into_error()),
                    cache::Event::File(file) => {
                        if let Some(error) = &file.error {
                            crate::logging::record_error(&error.clone().into_error());
                        }
                    }
                    _ => {}
                }
                output.send(event)
            },
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    server: PathBuf,
    selection: SelectionInput,
    writing: WriteArgs,
}

pub(super) fn run(
    ssh: SshArgs,
    server: PathBuf,
    selection: Selection,
    writing: WriteArgs,
    sources: SourcesArgs,
    display: DisplayArgs,
) -> Result<()> {
    let mut completion = Completion::new(&writing, &sources.selected()?)?;
    let mut session = ssh.connect()?;
    let mut output = Presentation::remote(display);
    let mut environment = None;
    session
        .submit(
            Cache::ID,
            serde_json::to_value(Request {
                server,
                selection: SelectionInput::from_selection(selection)?,
                writing,
            })?,
        )?
        .collect(|reply| {
            if environment.is_none() {
                environment = Some(reply.decode::<HostEnvironment>()?.into_runtime());
                return Ok(());
            }
            completion.accept(
                reply
                    .decode()
                    .context("invalid remote server mod cache contents")?,
                &mut |event| output.event(event),
            )
        })?;
    let environment = environment.context("missing remote cache environment")?;
    completion.finish(
        crate::commands::source_support::identify::Location::Remote(&session, &environment),
        |event| output.event(event),
    )?;
    session.close()?;
    output.finish()
}
fn init() -> Result<()> {
    crate::remote::registry::register::<Cache>()
}
inventory::submit! { crate::initcall::Initcall(init) }
