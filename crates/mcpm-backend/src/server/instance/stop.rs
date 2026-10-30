//! `server.instance.stop`: request graceful stop and force escalation.

use std::{future::Future, path::PathBuf};

use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::{
    data::server::instance::{self, InstancePaths, InstanceStatus, protocol::Frame},
    error::OperationError,
};

#[derive(Debug)]
pub struct Stop;

crate::host_operation!(Stop, "server.instance.stop");

impl Operation for Stop {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = StopInput;
    type Event = ();
    type Output = StopOutput;
    type Error = OperationError<StopKind>;

    fn run(
        input: StopInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || stop(input))
                .await
                .map_err(|error: StopFailure| error.into_operation())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopInput {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopOutput {
    pub status: InstanceStatus,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum StopKind {
    Connect,
    Protocol,
    State,
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

fn stop(input: StopInput) -> Result<StopOutput, StopFailure> {
    let paths = InstancePaths::new(input.path);
    let state = instance::state::read(&paths).map_err(StopFailure::State)?;
    if state
        .as_ref()
        .is_none_or(|state| !matches!(state.status, InstanceStatus::Running))
    {
        return Ok(StopOutput {
            status: InstanceStatus::Stopped,
        });
    }
    let address = mcpm_exec::ipc::SocketAddress::for_identity(paths.socket_identity());
    #[cfg(unix)]
    let mut stream = mcpm_exec::ipc::LocalStream::Unix(
        address
            .connect()
            .map_err(|error| StopFailure::Connect(anyhow::anyhow!(error)))?,
    );
    #[cfg(windows)]
    let mut stream = mcpm_exec::ipc::LocalStream::NamedPipe(
        address
            .connect_named_pipe()
            .map_err(|error| StopFailure::Connect(anyhow::anyhow!(error)))?,
    );
    instance::protocol::write(&mut stream, &Frame::Stop { force: input.force })
        .map_err(StopFailure::Protocol)?;
    Ok(StopOutput {
        status: InstanceStatus::Stopped,
    })
}

#[derive(Debug, thiserror::Error)]
enum StopFailure {
    #[error("failed to connect to instance shim")]
    Connect(#[source] anyhow::Error),
    #[error("instance shim protocol failed")]
    Protocol(#[source] std::io::Error),
    #[error("failed to read instance state")]
    State(#[source] anyhow::Error),
}

impl StopFailure {
    fn into_operation(self) -> OperationError<StopKind> {
        let kind = match self {
            Self::Connect(_) => StopKind::Connect,
            Self::Protocol(_) => StopKind::Protocol,
            Self::State(_) => StopKind::State,
        };
        OperationError::from_source(kind, self)
    }
}
