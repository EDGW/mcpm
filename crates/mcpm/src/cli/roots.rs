//! Root-specific option composition using the existing server, client and tools schemas.

use clap::Args;

use super::{CommonArgs, Invocation, RemoteLogArgs, Root};
use crate::{
    commands::{Command, client::ClientArgs, server::ServerArgs},
    ssh::SshArgs,
};

/// Direct server commands and their SSH presentation options.
#[derive(Args, Debug)]
pub(super) struct ServerRootArgs {
    #[command(flatten)]
    ssh: SshArgs,
    #[command(flatten)]
    logs: RemoteLogArgs,
    #[command(flatten)]
    server: ServerArgs,
}

impl Root for ServerRootArgs {
    const ABOUT: &'static str = "Inspect and manage Minecraft servers";

    fn into_invocation(self, common: CommonArgs) -> Invocation {
        Invocation {
            common,
            command: Command::Server(self.server),
            ssh: self.ssh,
            remote_logs: self.logs.remote_logs,
        }
    }
}

/// Direct local client commands without remote options.
#[derive(Args, Debug)]
pub(super) struct ClientRootArgs {
    #[command(flatten)]
    client: ClientArgs,
}

impl Root for ClientRootArgs {
    const ABOUT: &'static str = "Inspect and launch local Minecraft clients";

    fn into_invocation(self, common: CommonArgs) -> Invocation {
        Invocation {
            common,
            command: Command::Client(self.client),
            ssh: SshArgs::default(),
            remote_logs: false,
        }
    }
}

/// The complete tools command tree and its SSH presentation options.
#[derive(Args, Debug)]
pub(super) struct ToolsRootArgs {
    #[command(flatten)]
    ssh: SshArgs,
    #[command(flatten)]
    logs: RemoteLogArgs,
    #[command(subcommand)]
    command: Command,
}

impl Root for ToolsRootArgs {
    const ABOUT: &'static str = "Inspect and manage Minecraft clients, servers and mods";

    fn into_invocation(self, common: CommonArgs) -> Invocation {
        Invocation {
            common,
            command: self.command,
            ssh: self.ssh,
            remote_logs: self.logs.remote_logs,
        }
    }
}
