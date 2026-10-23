//! Server-owned remote compatibility defaults; providers and credentials remain on the client.

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Compatibility;
use crate::{
    remote::command::{Command, Output},
    ssh::SshArgs,
};

pub(super) const SEARCH: &str = "server.mods.source.search";
pub(super) const FILES: &str = "server.mods.source.files";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    server: PathBuf,
}

struct Search;
impl Command for Search {
    const ID: &'static str = SEARCH;
    fn run(contents: Value, output: &Output) -> Result<()> {
        serve(contents, output)
    }
}
struct Files;
impl Command for Files {
    const ID: &'static str = FILES;
    fn run(contents: Value, output: &Output) -> Result<()> {
        serve(contents, output)
    }
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
fn serve(contents: Value, output: &Output) -> Result<()> {
    let args: Request =
        serde_json::from_value(contents).context("invalid server compatibility arguments")?;
    tracing::info!(server = %args.server.display(), "Loading remote server source compatibility");
    output.checkpoint()?;
    let defaults = Compatibility::load(args.server)?;
    output.checkpoint()?;
    tracing::debug!(?defaults, "Resolved remote server source compatibility");
    output.send(defaults)
}

pub(super) fn defaults(
    ssh: SshArgs,
    server: PathBuf,
    command: &'static str,
) -> Result<Compatibility> {
    tracing::info!(command, "Opening SSH data source for server compatibility");
    let mut session = ssh.connect()?;
    let mut defaults = None;
    session
        .submit(command, serde_json::to_value(Request { server })?)?
        .collect(|reply| {
            ensure!(
                defaults.is_none(),
                "duplicate remote server compatibility result"
            );
            defaults = Some(
                reply
                    .decode()
                    .context("invalid remote server compatibility result")?,
            );
            Ok(())
        })?;
    session.close()?;
    defaults.context("missing remote server compatibility result")
}

fn init() -> Result<()> {
    crate::remote::registry::register::<Search>()?;
    crate::remote::registry::register::<Files>()
}
inventory::submit! { crate::initcall::Initcall(init) }
