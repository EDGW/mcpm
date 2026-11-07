//! Shared link control envelopes and typed client operation identifiers.

use crate::error::ErrorReport;
use crate::operation::{Operation, StreamingOutput};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Exact build identity shared by clients, links, and remote daemons.
pub const VERSION: &str = env!("MCPM_BUILD_VERSION");

#[derive(Debug, Serialize, Deserialize)]
pub struct HelloInfo {
    pub version: String,
    pub operations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LinkStatus {
    pub connections: usize,
    pub calls: usize,
    pub idle_seconds: u64,
    pub ssh_alive: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StopInput {
    pub force: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogInput {
    pub level: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRecord {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// Startup warnings followed by one terminal response on the inherited readiness pipe.
#[derive(Debug, Serialize, Deserialize)]
pub enum Ready {
    Warning(String),
    Ready,
    AlreadyRunning,
    Error(ErrorReport),
}

/// The endpoint, rather than the local executor, owns context-dependent control operations.
#[derive(Debug, thiserror::Error)]
#[error("daemon control operation `{0}` requires a daemon session")]
struct DaemonSessionRequired(&'static str);

macro_rules! control {
    ($name:ident, $id:literal, $input:ty, $output:ty) => {
        pub struct $name;
        impl Operation for $name {
            const ID: &'static str = $id;
            type Input = $input;
            type Output = $output;
            async fn run(_: Self::Input) -> Result<Self::Output> {
                Err(DaemonSessionRequired(Self::ID).into())
            }
        }
    };
}

control!(Hello, "link.hello", (), HelloInfo);
control!(DaemonHello, "mcpmd.hello", (), HelloInfo);
control!(Status, "link.status", (), LinkStatus);
control!(Stop, "link.stop", StopInput, ());
control!(Logs, "mcpmd.logs", LogInput, StreamingOutput<LogRecord>);
