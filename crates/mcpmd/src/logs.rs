//! Structured tracing capture with independent streaming subscriptions and stderr diagnostics.

use anyhow::{Result, ensure};
use mcpm_exec::{
    operation::{Operation, StreamingOutput},
    service::control::{LogInput, LogRecord},
};
use std::{
    fmt::Write,
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;
use tracing::{
    Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

static HUB: LazyLock<broadcast::Sender<LogRecord>> = LazyLock::new(|| broadcast::channel(1024).0);

pub fn init() -> Result<()> {
    tracing_subscriber::registry()
        .with(Capture)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::io::stderr)
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize daemon logging: {error}"))
}

/// Link diagnostics go directly to its per-target log, including startup lifecycle events.
pub fn init_link(log: std::fs::File) -> Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(log))
        .with_max_level(tracing::Level::DEBUG)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize link logging: {error}"))
}

pub struct Logs;
impl Operation for Logs {
    const ID: &'static str = "mcpmd.logs";
    type Input = LogInput;
    type Output = StreamingOutput<LogRecord>;
    async fn run(input: LogInput) -> Result<Self::Output> {
        let selected = input.level.to_ascii_uppercase();
        ensure!(
            matches!(
                selected.as_str(),
                "OFF" | "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE"
            ),
            "invalid log level {}",
            input.level
        );
        let mut receive = HUB.subscribe();
        Ok(StreamingOutput::new(move |sender| async move {
            loop {
                match receive.recv().await {
                    Ok(record) if rank(&record.level) <= rank(&selected) => {
                        sender.send(record).await?
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        sender
                            .send(LogRecord {
                                timestamp: timestamp(),
                                level: "WARN".into(),
                                target: module_path!().into(),
                                message: format!(
                                    "{count} remote log records expired before delivery"
                                ),
                            })
                            .await?;
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }))
    }
}

fn rank(level: &str) -> u8 {
    match level {
        "OFF" => 0,
        "ERROR" => 1,
        "WARN" => 2,
        "INFO" => 3,
        "DEBUG" => 4,
        _ => 5,
    }
}
fn timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

struct Capture;
impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, context: Context<'_, S>) {
        if HUB.receiver_count() == 0 {
            return;
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                let _ = write!(fields.0, " [{}]", span.name());
            }
        }
        let _ = HUB.send(LogRecord {
            timestamp: timestamp(),
            level: event.metadata().level().as_str().into(),
            target: event.metadata().target().into(),
            message: fields.0,
        });
    }
}
struct Fields(String);
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }
}
