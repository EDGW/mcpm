//! `server.instance.send`: send one serialized input line to a running shim.

use std::{future::Future, path::PathBuf};

use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::{
    data::server::instance::{self, InstancePaths, protocol::Frame},
    error::OperationError,
};

#[derive(Debug)]
pub struct Send;

crate::host_operation!(Send, "server.instance.send");

impl Operation for Send {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = SendInput;
    type Event = ();
    type Output = ();
    type Error = OperationError<SendKind>;

    fn run(
        input: SendInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<(), Self::Error>> + std::marker::Send {
        async move {
            cx.blocking(move || send(input))
                .await
                .map_err(|error: SendFailure| error.into_operation())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendInput {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    pub line: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum SendKind {
    Connect,
    Protocol,
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

fn send(input: SendInput) -> Result<(), SendFailure> {
    let paths = InstancePaths::new(input.path);
    let address = mcpm_exec::ipc::SocketAddress::for_identity(paths.socket_identity());
    #[cfg(unix)]
    let mut stream = mcpm_exec::ipc::LocalStream::Unix(
        address
            .connect()
            .map_err(|error| SendFailure::Connect(anyhow::anyhow!(error)))?,
    );
    #[cfg(windows)]
    let mut stream = mcpm_exec::ipc::LocalStream::NamedPipe(
        address
            .connect_named_pipe()
            .map_err(|error| SendFailure::Connect(anyhow::anyhow!(error)))?,
    );
    instance::protocol::write(&mut stream, &Frame::Input { line: input.line })
        .map_err(SendFailure::Protocol)
}

#[derive(Debug, thiserror::Error)]
enum SendFailure {
    #[error("failed to connect to instance shim")]
    Connect(#[source] anyhow::Error),
    #[error("instance shim protocol failed")]
    Protocol(#[source] std::io::Error),
}

impl SendFailure {
    fn into_operation(self) -> OperationError<SendKind> {
        let kind = match self {
            Self::Connect(_) => SendKind::Connect,
            Self::Protocol(_) => SendKind::Protocol,
        };
        OperationError::from_source(kind, self)
    }
}
