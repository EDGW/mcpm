//! Server-cache request adapter; shared caching owns workflow, reports and validation.

use super::ServerMods;
use crate::{
    commands::mod_support::{
        caching::{
            self,
            args::WriteArgs,
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
        caching::run(
            selection,
            server.scope(),
            args.writing.options(server.loaders())?,
            || output.checkpoint(),
            |event| {
                match &event {
                    caching::Event::Selection(
                        crate::commands::mod_support::scan::Event::Diagnostic { error },
                    ) => crate::logging::record_error(&error.clone().into_error()),
                    caching::Event::File(file) => {
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
    display: DisplayArgs,
) -> Result<()> {
    let mut session = ssh.connect()?;
    let mut output = Presentation::remote(display);
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
            output.event(
                reply
                    .decode()
                    .context("invalid remote server mod cache contents")?,
            )
        })?;
    session.close()?;
    output.finish()
}
fn init() -> Result<()> {
    crate::remote::registry::register::<Cache>()
}
inventory::submit! { crate::initcall::Initcall(init) }
