//! Transport models shared by Host operations, private shim IPC and terminal flows.

use mcpm_lib::local::mc::server::{
    authlib_injector::JarPolicy, config::ConfigurationOverrides, launch::LaunchPlan,
};
pub use mcpm_lib::local::mc::server::launch::LaunchExit;
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
    pub exit: Option<ExitReason>,
    pub communication: Communication,
    pub actual_channel: Channel,
    /// Communication evidence, independent of Java's lifecycle and command transport.
    pub server_status: ServerStatus,
    pub stdout: CursorRange,
    pub stderr: CursorRange,
}

/// Classified termination reason of the managed Java process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExitReason {
    /// Java exited cleanly with code 0.
    Exited,
    /// Java exited with a non-zero exit code or a non-`SIGKILL` signal.
    Crashed { code: u32 },
    /// Java was terminated by mcpm (`@kill` / `Kill` RPC).
    KilledByMcpm { exit: LaunchExit },
    /// Java was terminated externally by `SIGKILL`.
    KilledByUser,
}

impl ExitReason {
    /// POSIX `SIGKILL` signal number, invariant across Unix hosts and cross-OS clients.
    pub const SIGKILL: i32 = 9;

    /// Classifies Java's process exit from the raw wait outcome and whether mcpm invoked kill.
    pub fn from_launch_exit(exit: LaunchExit, killed_by_mcpm: bool) -> Self {
        match exit {
            LaunchExit::Code(0) => Self::Exited,
            LaunchExit::Signal(Self::SIGKILL) if killed_by_mcpm => Self::KilledByMcpm { exit },
            LaunchExit::Signal(Self::SIGKILL) => Self::KilledByUser,
            LaunchExit::Code(_) if killed_by_mcpm => Self::KilledByMcpm { exit },
            LaunchExit::Code(code) => Self::Crashed { code },
            LaunchExit::Signal(_) => Self::Crashed {
                code: exit.shell_code(),
            },
        }
    }

    /// Fallback classification when reading legacy receipts that only stored `exit_code`.
    pub(crate) fn from_legacy_code(code: u32) -> Self {
        if code == 0 {
            Self::Exited
        } else {
            Self::Crashed { code }
        }
    }

    /// Shell-compatible exit code propagated by `mcpm attach` / `mcpm launch`.
    pub fn shell_code(self) -> u32 {
        match self {
            Self::Exited => 0,
            Self::Crashed { code } => code,
            Self::KilledByMcpm { exit } => exit.shell_code(),
            Self::KilledByUser => LaunchExit::Signal(Self::SIGKILL).shell_code(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_java_exit_reasons_and_shell_codes() {
        assert_eq!(
            ExitReason::from_launch_exit(LaunchExit::Code(0), false),
            ExitReason::Exited
        );
        assert_eq!(
            ExitReason::from_launch_exit(LaunchExit::Code(0), true),
            ExitReason::Exited
        );
        assert_eq!(ExitReason::Exited.shell_code(), 0);

        let mcpm_unix =
            ExitReason::from_launch_exit(LaunchExit::Signal(ExitReason::SIGKILL), true);
        assert_eq!(
            mcpm_unix,
            ExitReason::KilledByMcpm {
                exit: LaunchExit::Signal(ExitReason::SIGKILL),
            }
        );
        assert_eq!(mcpm_unix.shell_code(), 137);

        let mcpm_windows = ExitReason::from_launch_exit(LaunchExit::Code(1), true);
        assert_eq!(
            mcpm_windows,
            ExitReason::KilledByMcpm {
                exit: LaunchExit::Code(1),
            }
        );
        assert_eq!(mcpm_windows.shell_code(), 1);

        let user_kill =
            ExitReason::from_launch_exit(LaunchExit::Signal(ExitReason::SIGKILL), false);
        assert_eq!(user_kill, ExitReason::KilledByUser);
        assert_eq!(user_kill.shell_code(), 137);

        let crashed_code = ExitReason::from_launch_exit(LaunchExit::Code(37), false);
        assert_eq!(crashed_code, ExitReason::Crashed { code: 37 });
        assert_eq!(crashed_code.shell_code(), 37);

        let crashed_sigterm = ExitReason::from_launch_exit(LaunchExit::Signal(15), false);
        assert_eq!(crashed_sigterm, ExitReason::Crashed { code: 143 });
        assert_eq!(crashed_sigterm.shell_code(), 143);

        let crashed_sigsegv_racing_kill =
            ExitReason::from_launch_exit(LaunchExit::Signal(11), true);
        assert_eq!(
            crashed_sigsegv_racing_kill,
            ExitReason::Crashed { code: 139 }
        );
    }
}
