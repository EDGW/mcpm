//! `server.instance.attach`: observe a running shim and stream console output.

use std::{future::Future, io::BufReader, path::PathBuf};

use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::{
    data::server::instance::{self, InstancePaths, InstanceStatus, protocol::Frame},
    error::OperationError,
};

#[derive(Debug)]
pub struct Attach;

crate::host_operation!(Attach, "server.instance.attach");

impl Operation for Attach {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = AttachInput;
    type Event = AttachEvent;
    type Output = AttachOutput;
    type Error = OperationError<AttachKind>;

    fn run(
        input: AttachInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        let emitter = cx.emitter();
        async move {
            cx.blocking(move || attach(input, emitter))
                .await
                .map_err(|error: AttachFailure| error.into_operation())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachInput {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    #[serde(default)]
    pub tail: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachEvent {
    pub sequence: u64,
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachOutput {
    pub status: InstanceStatus,
    pub exit: Option<crate::data::server::instance::ExitRecord>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum AttachKind {
    Connect,
    Protocol,
    State,
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

fn attach(
    input: AttachInput,
    emitter: mcpm_exec::Emitter<AttachEvent>,
) -> Result<AttachOutput, AttachFailure> {
    let paths = InstancePaths::new(input.path);
    let state = instance::state::read(&paths).map_err(AttachFailure::State)?;
    if state
        .as_ref()
        .is_none_or(|state| !matches!(state.status, InstanceStatus::Running))
    {
        return Ok(AttachOutput {
            status: state
                .as_ref()
                .map_or(InstanceStatus::Stopped, |state| state.status.clone()),
            exit: state.and_then(|state| state.exit),
        });
    }

    let address = mcpm_exec::ipc::SocketAddress::for_identity(paths.socket_identity());
    #[cfg(unix)]
    let mut stream = mcpm_exec::ipc::LocalStream::Unix(
        address
            .connect()
            .map_err(|error| AttachFailure::Connect(anyhow::anyhow!(error)))?,
    );
    #[cfg(windows)]
    let mut stream = mcpm_exec::ipc::LocalStream::NamedPipe(
        address
            .connect_named_pipe()
            .map_err(|error| AttachFailure::Connect(anyhow::anyhow!(error)))?,
    );

    instance::protocol::write(&mut stream, &Frame::Hello { tail: input.tail })
        .map_err(AttachFailure::Protocol)?;
    let reader_stream = stream.try_clone().map_err(AttachFailure::Protocol)?;
    let mut reader = BufReader::new(reader_stream);
    while let Some(frame) =
        instance::protocol::read(&mut reader).map_err(AttachFailure::Protocol)?
    {
        match frame {
            Frame::Missing { .. } => {}
            Frame::Line {
                sequence,
                stream,
                text,
            } => emitter
                .emit(AttachEvent {
                    sequence,
                    stream,
                    text,
                })
                .map_err(|_| AttachFailure::Protocol(std::io::Error::other("attach cancelled")))?,
            Frame::Exit {
                code,
                signal,
                stopped,
            } => {
                return Ok(AttachOutput {
                    status: if stopped {
                        InstanceStatus::Stopped
                    } else {
                        InstanceStatus::Exited
                    },
                    exit: Some(crate::data::server::instance::ExitRecord {
                        code,
                        signal,
                        stopped,
                        at: instance::state::now(),
                    }),
                });
            }
            Frame::Bye => break,
            _ => {}
        }
    }
    Ok(AttachOutput {
        status: InstanceStatus::Running,
        exit: None,
    })
}

#[derive(Debug, thiserror::Error)]
enum AttachFailure {
    #[error("failed to connect to instance shim")]
    Connect(#[source] anyhow::Error),
    #[error("instance shim protocol failed")]
    Protocol(#[source] std::io::Error),
    #[error("failed to read instance state")]
    State(#[source] anyhow::Error),
}

impl AttachFailure {
    fn into_operation(self) -> OperationError<AttachKind> {
        let kind = match self {
            Self::Connect(_) => AttachKind::Connect,
            Self::Protocol(_) => AttachKind::Protocol,
            Self::State(_) => AttachKind::State,
        };
        OperationError::from_source(kind, self)
    }
}
