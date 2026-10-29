//! Per-call log capture: tracing events reach the call whose span carries its GUID.
//!
//! Serve wraps every request in a span with a `guid` field and registers a route for that
//! call. The layer installed by the process entry point forwards matching events, filtered
//! by the call's requested level, and drops them rather than blocking when the writer is
//! behind.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{LazyLock, Mutex},
};

use tokio::sync::mpsc;
use tracing::{
    Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};
use uuid::Uuid;

use crate::protocol::{Frame, LogLevel, LogRecord, LogSpan};

/// Span field that identifies the call a log record belongs to.
const GUID_FIELD: &str = "guid";

static ROUTES: LazyLock<Mutex<HashMap<Uuid, Route>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
struct Route {
    level: LogLevel,
    frames: mpsc::Sender<Frame>,
}

/// Forwards log records to one call until dropped.
#[derive(Debug)]
pub(crate) struct Subscription {
    guid: Uuid,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        ROUTES.lock().unwrap().remove(&self.guid);
    }
}

pub(crate) fn subscribe(
    guid: Uuid,
    level: LogLevel,
    frames: mpsc::Sender<Frame>,
) -> Subscription {
    ROUTES.lock().unwrap().insert(guid, Route { level, frames });
    Subscription { guid }
}

/// Layer that routes tracing events to the call whose span scope contains its GUID.
///
/// The process entry point installs it next to its own layers; without it serve sends no
/// log frames.
#[derive(Debug, Default)]
pub struct Capture;

impl<S> Layer<S> for Capture
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        if let Some(span) = context.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(
        &self,
        id: &tracing::Id,
        values: &tracing::span::Record<'_>,
        context: Context<'_, S>,
    ) {
        if let Some(span) = context.span(id)
            && let Some(fields) = span.extensions_mut().get_mut::<Fields>()
        {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, context: Context<'_, S>) {
        let Some(guid) = call(event, &context) else {
            return;
        };
        let routes = ROUTES.lock().unwrap();
        let Some(route) = routes.get(&guid) else {
            return;
        };
        if !route.level.allows(event.metadata().level()) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let spans = context
            .event_scope(event)
            .into_iter()
            .flat_map(|scope| scope.from_root())
            .map(|span| {
                LogSpan::new(
                    span.name(),
                    span.extensions()
                        .get::<Fields>()
                        .map(|fields| fields.0.clone())
                        .unwrap_or_default(),
                )
            })
            .collect();
        let record = LogRecord::new(
            level(event.metadata().level()),
            event.metadata().target(),
            fields.0,
            spans,
        );
        // Logging never blocks the operation that produced the record.
        let _ = route.frames.try_send(Frame::log(guid, record));
    }
}

/// The GUID of the innermost span of the event's scope that carries one.
fn call<S>(event: &tracing::Event<'_>, context: &Context<'_, S>) -> Option<Uuid>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let scope = context.event_scope(event)?;
    scope.from_root().find_map(|span| {
        let guid = span
            .extensions()
            .get::<Fields>()
            .and_then(|fields| fields.0.get(GUID_FIELD))
            .cloned()?;
        Uuid::parse_str(&guid).ok()
    })
}

fn level(level: &tracing::Level) -> LogLevel {
    match *level {
        tracing::Level::ERROR => LogLevel::Error,
        tracing::Level::WARN => LogLevel::Warn,
        tracing::Level::INFO => LogLevel::Info,
        tracing::Level::DEBUG => LogLevel::Debug,
        tracing::Level::TRACE => LogLevel::Trace,
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
