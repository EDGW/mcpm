mod writer;

use anyhow::Result;
use clap::ValueEnum;
use tracing_subscriber::filter::LevelFilter;

use crate::terminal;
pub(crate) use writer::{ProgressGuard, attach_progress};

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

pub(crate) fn init(level: LogLevel) -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(level.filter())
        .with_ansi(terminal::stderr_color_enabled())
        .with_writer(writer::WriterFactory)
        .with_target(false)
        .without_time()
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize diagnostic logging: {error}"))
}
