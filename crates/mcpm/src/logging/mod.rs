//! Independent terminal and per-process file logging, including user-facing error chains.
//!
//! This module also owns the process's log level, the remote-record replay used by the SSH
//! session and the check that keeps a serve process from drawing on the caller's terminal.

use std::{
    fs::File,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

use anyhow::Result;
use clap::ValueEnum;
use mcpm_exec::protocol::{LogLevel as ProtocolLevel, LogRecord};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{
    Layer,
    filter::{LevelFilter, filter_fn},
    fmt::MakeWriter,
    prelude::*,
};

use crate::terminal;

pub(crate) use writer::{ProgressGuard, attach_progress, pause_console};

mod file;
mod format;
mod writer;

const REMOTE_TARGET: &str = "mcpm::remote_log";
const DIAGNOSTIC_TARGET: &str = "mcpm::diagnostics";

static LEVEL: OnceLock<LogLevel> = OnceLock::new();
static REMOTE_LOGS: OnceLock<bool> = OnceLock::new();
static SERVING: OnceLock<bool> = OnceLock::new();

pub(crate) fn level() -> LogLevel {
    LEVEL.get().copied().unwrap_or(LogLevel::Warn)
}

fn remote_logs_enabled() -> bool {
    REMOTE_LOGS.get().copied().unwrap_or(false)
}

/// Whether this process serves a remote session, where stderr belongs to the protocol peer.
pub(crate) fn is_serving() -> bool {
    SERVING.get().copied().unwrap_or(false)
}

pub(crate) fn init(
    level: LogLevel,
    started: SystemTime,
    remote: bool,
    remote_logs: bool,
) -> Result<()> {
    let (file, path) = file::open(started)?;
    let _ = LEVEL.set(level);
    let _ = REMOTE_LOGS.set(remote_logs);
    let _ = SERVING.set(remote);
    subscriber(level, writer::WriterFactory, file, remote)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize diagnostic logging: {error}"))?;
    tracing::debug!(version = crate::build_info::VERSION, pid = std::process::id(), log_file = %path.display(), "mcpm started");
    Ok(())
}

fn subscriber<W>(
    level: LogLevel,
    console: W,
    file: File,
    remote: bool,
) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let console = tracing_subscriber::fmt::layer()
        .with_ansi(terminal::stderr_color_enabled())
        .with_writer(console)
        .event_format(format::Console::default())
        .with_filter(if remote {
            LevelFilter::OFF
        } else {
            level.filter()
        })
        // diagnostics renders its themed error chain separately on stderr.
        .with_filter(filter_fn(|metadata| {
            metadata.target() != DIAGNOSTIC_TARGET
                && (metadata.target() != REMOTE_TARGET || remote_logs_enabled())
        }));
    let file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .with_filter(LevelFilter::DEBUG);
    tracing_subscriber::registry()
        .with(console)
        .with(file)
        .with(mcpm_exec::serve::logs::Capture.with_filter(if remote {
            LevelFilter::TRACE
        } else {
            LevelFilter::OFF
        }))
}

/// Renders a record forwarded by a remote serve process into the local sinks.
pub(crate) fn replay(mut record: LogRecord) {
    let message = record.take_message();
    let detail = format!("{}: {message}", record.target());
    let fields = record.fields();
    let spans = record.spans();
    macro_rules! emit {
        ($level:expr) => {
            tracing::event!(
                target: REMOTE_TARGET,
                $level,
                remote_fields = ?fields,
                remote_spans = ?spans,
                "{detail}"
            )
        };
    }
    match record.level() {
        ProtocolLevel::Off => {}
        ProtocolLevel::Error => emit!(tracing::Level::ERROR),
        ProtocolLevel::Warn => emit!(tracing::Level::WARN),
        ProtocolLevel::Info => emit!(tracing::Level::INFO),
        ProtocolLevel::Debug => emit!(tracing::Level::DEBUG),
        ProtocolLevel::Trace => emit!(tracing::Level::TRACE),
    }
}

pub(crate) fn record_error(error: &anyhow::Error) {
    tracing::error!(target: DIAGNOSTIC_TARGET, error = ?kako_mc_lib::error::ErrorReport::capture(error), "operation failed");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn filter(self) -> LevelFilter {
        match self {
            Self::Off => LevelFilter::OFF,
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }
}

impl From<LogLevel> for ProtocolLevel {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Off => Self::Off,
            LogLevel::Error => Self::Error,
            LogLevel::Warn => Self::Warn,
            LogLevel::Info => Self::Info,
            LogLevel::Debug => Self::Debug,
            LogLevel::Trace => Self::Trace,
        }
    }
}
