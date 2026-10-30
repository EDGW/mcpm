//! `server.instance.status`: inspect persisted shim state.

use crate::{
    data::server::instance::{self, InstancePaths, InstanceState, InstanceStatus},
    error::OperationError,
};
use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf};
#[derive(Debug)]
pub struct Status;
crate::host_operation!(Status, "server.instance.status");
impl Operation for Status {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = StatusInput;
    type Event = ();
    type Output = StatusOutput;
    type Error = OperationError<StatusKind>;
    fn run(
        input: StatusInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || status(input))
                .await
                .map_err(|e: StatusFailure| e.into_operation())
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInput {
    #[serde(default = "default_path")]
    pub path: PathBuf,
}
fn default_path() -> PathBuf {
    PathBuf::from(".")
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusOutput {
    pub status: InstanceStatus,
    pub state: Option<InstanceState>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    State,
}
fn status(i: StatusInput) -> Result<StatusOutput, StatusFailure> {
    let p = InstancePaths::new(i.path);
    let state = instance::state::read(&p).map_err(StatusFailure::State)?;
    let status = if let Some(s) = &state {
        s.status.clone()
    } else {
        InstanceStatus::Stopped
    };
    Ok(StatusOutput { status, state })
}
#[derive(Debug, thiserror::Error)]
enum StatusFailure {
    #[error("failed to read instance state")]
    State(#[source] anyhow::Error),
}
impl StatusFailure {
    fn into_operation(self) -> OperationError<StatusKind> {
        OperationError::from_source(StatusKind::State, self)
    }
}
