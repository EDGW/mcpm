//! `server.instance.start`: prepare a host plan and launch a detached shim.
use super::shim::StartSpec;
use crate::{
    data::server::{authlib::JarPolicy, config::ConfigurationOverrides, launch},
    error::OperationError,
};
use anyhow::Result;
use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf};

#[derive(Debug)]
pub struct Start;
crate::host_operation!(Start, "server.instance.start");
impl Operation for Start {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = StartInput;
    type Event = StartEvent;
    type Output = StartOutput;
    type Error = OperationError<StartKind>;
    fn run(
        input: StartInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        async move {
            cx.blocking(move || start(input))
                .await
                .map_err(|e: StartFailure| e.into_operation())
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartInput {
    #[serde(default = "default_path")]
    pub path: PathBuf,
    #[serde(default)]
    pub overrides: ConfigurationOverridesWire,
    pub auth_api: Option<String>,
    pub auth_jar_policy: Option<JarPolicy>,
    #[serde(default)]
    pub stop_timeout_secs: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigurationOverridesWire {
    #[serde(default)]
    pub assignments: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StartEvent {
    Prepared,
    Starting,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartOutput {
    pub instance_id: String,
    pub status: crate::data::server::instance::InstanceStatus,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartKind {
    AlreadyRunning,
    Preparation,
    Spawn,
    Handshake,
}
fn default_path() -> PathBuf {
    PathBuf::from(".")
}
fn start(input: StartInput) -> Result<StartOutput, StartFailure> {
    let path = std::fs::canonicalize(&input.path)
        .map_err(|e| StartFailure::Preparation(anyhow::Error::from(e)))?;
    let mut overrides = ConfigurationOverrides::default();
    for a in input.overrides.assignments {
        overrides
            .parse(&a)
            .map_err(|e| StartFailure::Preparation(anyhow::Error::from(e)))?;
    }
    let plan = tokio::runtime::Handle::current()
        .block_on(launch::prepare_with_overrides(
            &path,
            &overrides,
            input.auth_jar_policy,
            || {
                kako_mc_lib::download::sources::downloader(
                    &kako_mc_lib::download::DownloadConfig::default(),
                )
            },
        ))
        .map_err(StartFailure::Preparation)?;
    let id = uuid::Uuid::new_v4().to_string();
    let spec = StartSpec {
        executable: plan.executable().to_owned(),
        directory: plan.directory().to_owned(),
        environment: plan.environment().clone(),
        arguments: plan.arguments().to_owned(),
        argument_file: plan.argument_file(),
        server_root: path.clone(),
        instance_id: id.clone(),
        plan: blake3::hash(plan.arguments().join("\0").as_bytes())
            .to_hex()
            .to_string(),
        stop_timeout_secs: input.stop_timeout_secs.max(10),
    };
    let exe = std::env::var_os("MCPMD_SHIM").unwrap_or_else(|| "mcpmd".into());
    let mut child = mcpm_exec::ipc::Detached::new(exe)
        .arg("shim")
        .spawn()
        .map_err(|e| StartFailure::Spawn(anyhow::Error::from(e)))?;
    let bytes =
        serde_json::to_vec(&spec).map_err(|e| StartFailure::Handshake(anyhow::Error::from(e)))?;
    child
        .startup
        .write_frame(&bytes)
        .map_err(|e| StartFailure::Handshake(anyhow::Error::from(e)))?;
    child
        .startup
        .write_frame(b"\n")
        .map_err(|e| StartFailure::Handshake(anyhow::Error::from(e)))?;
    child.startup.close_input();
    let frame = child
        .startup
        .read_frame(64 * 1024)
        .map_err(|e| StartFailure::Handshake(anyhow::Error::from(e)))?;
    let _ = child.child;
    if frame.starts_with(b"started") || frame.starts_with(b"{\"") {
        Ok(StartOutput {
            instance_id: id,
            status: crate::data::server::instance::InstanceStatus::Running,
        })
    } else {
        Err(StartFailure::Handshake(anyhow::anyhow!(
            String::from_utf8_lossy(&frame).to_string()
        )))
    }
}
#[derive(Debug, thiserror::Error)]
enum StartFailure {
    #[error("failed to prepare server instance")]
    Preparation(#[source] anyhow::Error),
    #[error("failed to spawn instance shim")]
    Spawn(#[source] anyhow::Error),
    #[error("instance shim startup handshake failed")]
    Handshake(#[source] anyhow::Error),
}
impl StartFailure {
    fn into_operation(self) -> OperationError<StartKind> {
        let kind = match self {
            Self::Preparation(_) => StartKind::Preparation,
            Self::Spawn(_) => StartKind::Spawn,
            Self::Handshake(_) => StartKind::Handshake,
        };
        OperationError::from_source(kind, self)
    }
}
