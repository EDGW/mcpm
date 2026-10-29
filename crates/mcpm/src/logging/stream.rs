//! Structured tracing capture and replay, independent of transports and command dispatch.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, OnceLock, mpsc},
};

use serde::{Deserialize, Serialize};
use tracing::{
    Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

use super::LogLevel;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    level: LogLevel,
    target: String,
    fields: BTreeMap<String, String>,
    spans: Vec<Span>,
}

impl Record {
    /// Preserve severity while making the remote origin explicit in every sink.
    pub(crate) fn replay(mut self) {
        let message = self.fields.remove("message").unwrap_or_default();
        let detail = format!("{}: {message}", self.target);
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!(target: super::REMOTE_TARGET, $level, remote_fields = ?self.fields, remote_spans = ?self.spans, "{detail}")
            };
        }
        match self.level {
            LogLevel::Off => {}
            LogLevel::Error => emit!(tracing::Level::ERROR),
            LogLevel::Warn => emit!(tracing::Level::WARN),
            LogLevel::Info => emit!(tracing::Level::INFO),
            LogLevel::Debug => emit!(tracing::Level::DEBUG),
            LogLevel::Trace => emit!(tracing::Level::TRACE),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Span {
    name: String,
    fields: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct Hub {
    sink: Option<(LogLevel, mpsc::Sender<Record>)>,
    startup: VecDeque<Record>,
    skipped: usize,
}

fn hub() -> &'static Mutex<Hub> {
    static HUB: OnceLock<Mutex<Hub>> = OnceLock::new();
    HUB.get_or_init(Mutex::default)
}

/// A subscription owns capture until shutdown; the receiver is drained by its transport.
#[derive(Debug)]
pub(crate) struct Subscription;

impl Subscription {
    pub(crate) fn start(level: LogLevel) -> (Self, mpsc::Receiver<Record>) {
        // File logging always keeps debug detail, independent of the console selection.
        let level = if level.filter() < LogLevel::Debug.filter() {
            LogLevel::Debug
        } else {
            level
        };
        let (send, receive) = mpsc::channel();
        let mut hub = hub().lock().unwrap();
        for record in hub.startup.drain(..) {
            if accepts(level, record.level) {
                let _ = send.send(record);
            }
        }
        let skipped = std::mem::take(&mut hub.skipped);
        if skipped > 0 && accepts(level, LogLevel::Warn) {
            let _ = send.send(Record {
                level: LogLevel::Warn,
                target: module_path!().into(),
                fields: BTreeMap::from([(
                    "message".into(),
                    format!("{skipped} startup log records expired before subscription"),
                )]),
                spans: Vec::new(),
            });
        }
        hub.sink = Some((level, send));
        (Self, receive)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        hub().lock().unwrap().sink.take();
    }
}

pub(crate) fn is_forwarding() -> bool {
    hub().lock().unwrap().sink.is_some()
}

fn accepts(selected: LogLevel, event: LogLevel) -> bool {
    event != LogLevel::Off && event.filter() <= selected.filter()
}

#[derive(Debug, Default)]
pub(super) struct Capture;

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        ctx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(&self, id: &tracing::Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(fields) = span.extensions_mut().get_mut::<Fields>()
        {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        // Replayed events must never be forwarded back through another connection.
        if event.metadata().target() == super::REMOTE_TARGET {
            return;
        }
        let level = match *event.metadata().level() {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        };
        let mut hub = hub().lock().unwrap();
        if let Some((selected, _)) = &hub.sink
            && !accepts(*selected, level)
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let spans = ctx
            .event_scope(event)
            .into_iter()
            .flat_map(|scope| scope.from_root())
            .map(|span| Span {
                name: span.name().into(),
                fields: span
                    .extensions()
                    .get::<Fields>()
                    .map(|fields| fields.0.clone())
                    .unwrap_or_default(),
            })
            .collect();
        let record = Record {
            level,
            target: event.metadata().target().into(),
            fields: fields.0,
            spans,
        };
        if let Some((_, send)) = &hub.sink {
            let _ = send.send(record);
        } else {
            // Bound startup storage for callers that never subscribe to logs.
            if hub.startup.len() == 256 {
                hub.startup.pop_front();
                hub.skipped += 1;
            }
            hub.startup.push_back(record);
        }
    }
}

#[derive(Debug, Default)]
struct Fields(BTreeMap<String, String>);
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
}
