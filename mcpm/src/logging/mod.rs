//! Independent terminal and per-process file logging, including user-facing error chains.

use std::{
    fs::File,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

use anyhow::Result;
use clap::ValueEnum;
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
pub(crate) mod stream;
mod writer;

const REMOTE_TARGET: &str = "mcpm::remote_log";
const DIAGNOSTIC_TARGET: &str = "mcpm::diagnostics";

static LEVEL: OnceLock<LogLevel> = OnceLock::new();
static REMOTE_LOGS: OnceLock<bool> = OnceLock::new();

pub(crate) fn level() -> LogLevel {
    LEVEL.get().copied().unwrap_or(LogLevel::Warn)
}

fn remote_logs_enabled() -> bool {
    REMOTE_LOGS.get().copied().unwrap_or(false)
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
        .with(stream::Capture.with_filter(if remote {
            LevelFilter::TRACE
        } else {
            LevelFilter::OFF
        }))
}

pub(crate) fn record_error(error: &anyhow::Error) {
    tracing::error!(target: DIAGNOSTIC_TARGET, error = %format_args!("{error:#}"), "operation failed");
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
