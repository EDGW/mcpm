//! Transport models shared by Host operations, private shim IPC and terminal flows.

use mcpm_lib::local::mc::server::{
    authlib_injector::JarPolicy, config::ConfigurationOverrides, launch::LaunchPlan,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Opaque on the client: native paths and environment are decoded only on their Host.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct EncodedPlan(Vec<u8>);
impl EncodedPlan {
    pub(crate) fn encode(plan: &LaunchPlan) -> anyhow::Result<Self> {
        Ok(Self(serde_json::to_vec(plan)?))
    }
    pub(crate) fn decode(&self) -> anyhow::Result<LaunchPlan> {
        Ok(serde_json::from_slice(&self.0)?)
    }
}

#[derive(Serialize, Deserialize)]
pub struct PrepareInput {
    pub server: PathBuf,
    pub overrides: ConfigurationOverrides,
    pub auth_jar_policy: Option<JarPolicy>,
}
#[derive(Serialize, Deserialize)]
pub struct Prepared {
    pub server: PathBuf,
    pub plan: EncodedPlan,
}
#[derive(Serialize, Deserialize)]
pub enum PrepareEvent {
    Progress {
        message: String,
        bytes: Option<(u64, Option<u64>)>,
    },
    Prepared(Prepared),
}
#[derive(Serialize, Deserialize)]
pub struct LaunchInput {
    pub server: PathBuf,
    pub plan: EncodedPlan,
    pub communication: Communication,
    pub launch_request_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Launched {
    pub reference: ShimRef,
    pub initial_cursor: u64,
    pub state: State,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShimRef {
    pub server: PathBuf,
    pub run_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectInput {
    pub server: Option<PathBuf>,
    pub run_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub reference: ShimRef,
    pub state: State,
    pub exit_code: Option<u32>,
    pub communication: Communication,
    pub actual_channel: Channel,
    /// Communication evidence, independent of Java's lifecycle and command transport.
    pub server_status: ServerStatus,
    pub stdout: CursorRange,
    pub stderr: CursorRange,
}

/// One persistent status subscription, with event-driven snapshots and periodic liveness updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInput {
    pub reference: ShimRef,
    #[serde(default = "status_interval_ms")]
    pub interval_ms: u64,
}
impl StatusInput {
    pub const DEFAULT_INTERVAL_MS: u64 = 1000;
    pub fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.interval_ms.clamp(100, 60_000))
    }
    /// Allow scheduling/transport slack, but do not leave a silent half-open Channel unbounded.
    pub fn timeout(&self) -> std::time::Duration {
        self.interval() * 3 + std::time::Duration::from_secs(2)
    }
}
fn status_interval_ms() -> u64 {
    StatusInput::DEFAULT_INTERVAL_MS
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInput {
    pub reference: ShimRef,
    pub listener_id: String,
    pub visibility: CommandVisibility,
    pub command_id: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub status: CommandStatus,
    pub channel: Channel,
    pub output: Option<String>,
    pub result: CommandOutcome,
    pub diagnostic: Option<String>,
}

/// Internal help responses remain private to their requesting frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandVisibility {
    User,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandsInput {
    pub reference: ShimRef,
}

/// Ready establishes the live-only subscription boundary; Done follows final results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandEvent {
    Ready,
    Completed {
        listener_id: String,
        command_id: String,
        text: String,
        result: CommandResult,
    },
    Gap {
        missed: u64,
    },
    Done,
}

/// Maximum source read block and maximum payload in one log batch (1 MiB).
pub const MAX_LOG_BLOCK_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsInput {
    pub reference: ShimRef,
    pub source: Source,
    pub cursor: Option<u64>,
    pub tail: Option<usize>,
    pub max_bytes: usize,
    pub max_items: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBatch {
    pub batch: Vec<LogChunk>,
    pub next_cursor: u64,
    pub gap: Option<Gap>,
    pub finished: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogChunk {
    pub seq: u64,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub from: u64,
    pub to: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorRange {
    pub oldest: u64,
    pub latest: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlInput {
    pub reference: ShimRef,
    pub listener_id: String,
    pub command_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillResult {
    Accepted,
    AlreadyExited,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Communication {
    #[default]
    Auto,
    Rcon,
    Stdin,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Rcon,
    Stdin,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Stdout,
    Stderr,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Stopping,
    Killing,
    Exited,
}

/// Communication evidence from configured RCON, independent of the launch policy label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    Unknown,
    RconConnected,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Accepted,
    Exited,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
    Completed,
    Sent,
    Unknown,
}
