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

pub(crate) use writer::{
    ConsoleChannelGuard, ConsoleGuard, ProgressGuard, attach_progress, pause_console,
    redirect_console,
};

mod file;
mod format;
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

/// Replays daemon records through the remote console filter and process file sink.
pub(crate) async fn subscribe_executor(executor: &mcpm_exec::service::Executor) -> Result<()> {
    let selected = if level() == LogLevel::Trace {
        "trace"
    } else {
        "debug"
    };
    if let Some(mut logs) = executor.subscribe_logs(selected.into()).await? {
        tokio::spawn(async move {
            while let Some(record) = logs.next().await {
                let record = match record {
                    Ok(record) => record,
                    Err(error) => {
                        tracing::debug!(error = %error, "remote log subscription ended");
                        break;
                    }
                };
                let message = format!("{}: {}", record.target, record.message);
                match record.level.as_str() {
                    "ERROR" => tracing::error!(target: REMOTE_TARGET, "{message}"),
                    "WARN" => tracing::warn!(target: REMOTE_TARGET, "{message}"),
                    "INFO" => tracing::info!(target: REMOTE_TARGET, "{message}"),
                    "TRACE" => tracing::trace!(target: REMOTE_TARGET, "{message}"),
                    _ => tracing::debug!(target: REMOTE_TARGET, "{message}"),
                }
            }
        });
    }
    Ok(())
}

pub(crate) fn init(level: LogLevel, started: SystemTime, remote_logs: bool) -> Result<()> {
    let (file, path) = file::open(started)?;
    let _ = LEVEL.set(level);
    let _ = REMOTE_LOGS.set(remote_logs);
    subscriber(level, writer::WriterFactory, file)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize diagnostic logging: {error}"))?;
    tracing::debug!(version = crate::build_info::VERSION, pid = std::process::id(), log_file = %path.display(), "mcpm started");
    Ok(())
}

fn subscriber<W>(level: LogLevel, console: W, file: File) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let console = tracing_subscriber::fmt::layer()
        .with_ansi(terminal::stderr_color_enabled())
        .with_writer(console)
        .event_format(format::Console::default())
        .with_filter(level.filter())
        // diagnostics renders its themed error chain separately on stderr.
        .with_filter(filter_fn(|metadata| {
            metadata.target() != DIAGNOSTIC_TARGET
                && (metadata.target() != REMOTE_TARGET || remote_logs_enabled())
        }));
    let file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .with_filter(LevelFilter::DEBUG);
    tracing_subscriber::registry().with(console).with(file)
}

pub(crate) fn record_error(error: &anyhow::Error) {
    tracing::error!(target: DIAGNOSTIC_TARGET, error = ?mcpm_exec::error::ErrorReport::capture(error), "operation failed");
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
