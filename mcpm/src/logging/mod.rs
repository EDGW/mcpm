//! Independent terminal and per-process file logging, including user-facing error chains.

use std::{fs::File, sync::Mutex, time::SystemTime};

use anyhow::Result;
use clap::ValueEnum;
use tracing_subscriber::{
    Layer,
    filter::{LevelFilter, filter_fn},
    fmt::MakeWriter,
    prelude::*,
};

use crate::terminal;

pub(crate) use writer::{ProgressGuard, attach_progress};

mod file;
mod writer;

const DIAGNOSTIC_TARGET: &str = "mcpm::diagnostics";

pub(crate) fn init(level: LogLevel, started: SystemTime) -> Result<()> {
    let (file, path) = file::open(started)?;
    subscriber(level, writer::WriterFactory, file)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize diagnostic logging: {error}"))?;
    tracing::debug!(version = env!("CARGO_PKG_VERSION"), pid = std::process::id(), log_file = %path.display(), "mcpm started");
    Ok(())
}

fn subscriber<W>(level: LogLevel, console: W, file: File) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let console = tracing_subscriber::fmt::layer()
        .with_ansi(terminal::stderr_color_enabled())
        .with_writer(console)
        .with_target(false)
        .without_time()
        .compact()
        .with_filter(level.filter())
        // diagnostics renders its themed error chain separately on stderr.
        .with_filter(filter_fn(|metadata| metadata.target() != DIAGNOSTIC_TARGET));
    let file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .with_filter(LevelFilter::DEBUG);
    tracing_subscriber::registry().with(console).with(file)
}

pub(crate) fn record_error(error: &anyhow::Error) {
    tracing::error!(target: DIAGNOSTIC_TARGET, error = %format_args!("{error:#}"), "operation failed");
}

#[derive(Debug, Clone, Copy, ValueEnum)]
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

#[cfg(test)]
mod tests;
