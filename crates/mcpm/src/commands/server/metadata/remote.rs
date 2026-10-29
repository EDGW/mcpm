//! Register server.info and retrieve one metadata snapshot through an SSH session.

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{super::workspace, ServerInfo};
use crate::{
    remote::command::{Command, Output},
    ssh::SshArgs,
};

struct Info;
impl Command for Info {
    const ID: &'static str = "server.info";

    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    fn run(contents: Value, output: &Output) -> Result<()> {
        let args: Request =
            serde_json::from_value(contents).context("invalid server info arguments")?;
        output.checkpoint()?;
        tracing::info!(path = %args.path.display(), "Reading remote server metadata");
        let server = workspace::inspect_local(args.path)?;
        let info = ServerInfo::from_server(&server);
        tracing::debug!(name = %info.name, "Loaded remote server metadata");
        output.send(info)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default = "default_path")]
    path: PathBuf,
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(in crate::commands::server) fn load(ssh: SshArgs, path: PathBuf) -> Result<ServerInfo> {
    tracing::info!("Opening SSH data source for server information");
    let mut session = ssh.connect()?;
    tracing::debug!(path = %path.display(), "Requesting remote server metadata");
    let mut info = None;
    session
        .submit(Info::ID, serde_json::to_value(Request { path })?)?
        .collect(|reply| {
            ensure!(
                info.is_none(),
                "remote server info returned multiple snapshots"
            );
            info = Some(
                reply
                    .decode::<ServerInfo>()
                    .context("invalid remote server information")?,
            );
            Ok(())
        })?;
    let info = info.context("remote server info returned no snapshot")?;
    session.close()?;
    tracing::debug!(name = %info.name, "Received remote server metadata");
    Ok(info)
}

fn init() -> Result<()> {
    crate::remote::registry::register::<Info>()
}

inventory::submit! { crate::initcall::Initcall(init) }
