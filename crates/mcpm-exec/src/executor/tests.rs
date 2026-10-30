//! Decoding of remote call frames into typed events, output and failures.

// Test operations mirror the trait's `impl Future` signature so their shape stays explicit.
#![allow(clippy::manual_async_fn)]

use std::{
    future::Future,
    process::Command as ProcessCommand,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};
use tracing_subscriber::{Layer, layer::Context as LayerContext, prelude::*, registry::LookupSpan};
use uuid::Uuid;

use super::*;
use crate::{
    error::{DispatchError, Failure},
    operation::{Context, Operation, Placement, Wire},
    protocol::{self, LogLevel},
    transport::session,
};

struct StartLogLayer {
    records: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl<S> Layer<S> for StartLogLayer
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _context: LayerContext<'_, S>) {
        if *event.metadata().level() != tracing::Level::INFO {
            return;
        }
        let mut visitor = StartLogVisitor::default();
        event.record(&mut visitor);
        if visitor.message == "starting operation" {
            self.records.lock().unwrap().push((
                visitor.operation,
                visitor.placement,
                visitor.target,
            ));
        }
    }
}

#[derive(Default)]
struct StartLogVisitor {
    operation: String,
    placement: String,
    target: String,
    message: String,
}

impl tracing::field::Visit for StartLogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        match field.name() {
            "message" => self.message = value.trim_matches('"').to_owned(),
            "operation" => self.operation = value.trim_matches('"').to_owned(),
            "placement" => self.placement = value,
            "target" => self.target = value.trim_matches('"').to_owned(),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.record_debug(field, &value);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn operation_start_logs_once_with_dispatch_metadata() {
    let records = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(StartLogLayer {
        records: records.clone(),
    });
    tracing::subscriber::with_default(subscriber, || {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let executor = Executor::new(Provider, settings());
                let local = Session::new(executor.clone(), Target::Local);
                let _ = local.start::<Local>(()).await.unwrap().finish().await;
                let _ = local.start::<Echo>(1).await.unwrap().finish().await;

                let remote = Session::new(
                    executor,
                    Target::Remote(crate::ssh::Connection::new("ssh host", "")),
                );
                let _ = remote.start::<Echo>(1).await;
            });
        });
    });
    assert_eq!(
        *records.lock().unwrap(),
        vec![
            ("test.local".to_owned(), "Surface".to_owned(), "local".to_owned()),
            ("test.echo".to_owned(), "Host".to_owned(), "local".to_owned()),
            ("test.echo".to_owned(), "Host".to_owned(), "remote".to_owned()),
        ]
    );
}
/// Starts a session whose serve side replays `frames` (a shell snippet using `$guid`).
fn call(frames: &str) -> (session::Session, Handle<Echo>) {
    let script = format!(
        r#"
printf '%s\n' '{{"guid":"{nil}","status":"ready","contents":{{"version":{version},"build":"test-build","operations":["test.echo"]}}}}'
IFS= read -r request
guid=$(printf '%s' "$request" | cut -d '"' -f4)
{frames}
cat >/dev/null
printf '%s\n' '{{"guid":"{nil}","status":"closed","contents":null}}'
"#,
        nil = Uuid::nil(),
        version = protocol::VERSION,
    );
    let mut command = ProcessCommand::new("sh");
    command.args(["-c", &script]);
    let session = session::Session::start(
        &mut command,
        session::Options::new("test-build", LogLevel::Warn, std::sync::Arc::new(|_| {})),
    )
    .unwrap();
    let pending = session.submit(Echo::ID, json!(1)).unwrap();
    let handle = remote_handle(pending, Echo::WIRE.unwrap(), Default::default());
    (session, handle)
}

async fn collect(handle: Handle<Echo>) -> (Vec<Value>, Result<Value, Failure<EchoError>>) {
    let mut handle = handle;
    let mut events = Vec::new();
    while let Some(event) = handle.next().await {
        events.push(event);
    }
    let result = handle.finish().await;
    (events, result)
}

/// An operation that emits more events than the bounded queue holds must still finish when
/// the caller only asks for the result.
#[tokio::test(flavor = "multi_thread")]
async fn finish_drains_events_the_caller_never_read() {
    let session = Session::new(Executor::new(Provider, settings()), Target::Local);
    let total = 3 * crate::EVENT_CAPACITY as u64;
    let output = session.start::<Flood>(total).await.unwrap().finish().await;
    assert_eq!(output.unwrap(), json!({"emitted": total}));
}

/// The same over the protocol: the remote reader and the call's queues are bounded, so the
/// caller draining only at `finish` must still receive the whole stream's terminal result.
#[tokio::test(flavor = "multi_thread")]
async fn remote_finish_drains_events_the_caller_never_read() {
    let total = 3 * crate::EVENT_CAPACITY;
    let frames = (0..total)
        .map(|index| format!(r#"printf '{{"guid":"%s","status":"data","contents":{{"index":{index}}}}}\n' "$guid""#))
        .chain(std::iter::once(format!(
            r#"printf '{{"guid":"%s","status":"finished","contents":{{"emitted":{total}}}}}\n' "$guid""#
        )))
        .collect::<Vec<_>>()
        .join("\n");
    let (mut session, handle) = call(&frames);
    assert_eq!(
        handle.finish().await.unwrap(),
        json!({"emitted": total}),
        "a bounded call queue must not stop the operation from finishing"
    );
    session.close().unwrap();
}

/// A caller that reads late still receives every event in order: backpressure delays the
/// stream instead of dropping or buffering it without bound.
#[tokio::test(flavor = "multi_thread")]
async fn remote_events_arrive_in_order_after_a_delayed_caller() {
    let total = 3 * crate::EVENT_CAPACITY;
    let frames = (0..total)
        .map(|index| format!(r#"printf '{{"guid":"%s","status":"data","contents":{{"index":{index}}}}}\n' "$guid""#))
        .chain(std::iter::once(format!(
            r#"printf '{{"guid":"%s","status":"finished","contents":{{"emitted":{total}}}}}\n' "$guid""#
        )))
        .collect::<Vec<_>>()
        .join("\n");
    let (mut session, mut handle) = call(&frames);
    // Read nothing until the remote reader has certainly filled both bounded queues.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    for expected in 0..total {
        let event = handle.next().await.expect("every event must arrive");
        assert_eq!(event, json!({"index":expected}));
    }
    assert!(handle.next().await.is_none());
    assert_eq!(handle.finish().await.unwrap(), json!({"emitted": total}));
    session.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_and_output_are_decoded_from_frames() {
    let (mut session, handle) = call(
        r#"printf '{"guid":"%s","status":"data","contents":{"count":0}}\n' "$guid"
printf '{"guid":"%s","status":"finished","contents":{"count":1}}\n' "$guid""#,
    );
    let (events, result) = collect(handle).await;
    assert_eq!(events, [json!({"count":0})]);
    assert_eq!(result.unwrap(), json!({"count":1}));
    session.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn operation_failures_reach_the_caller_as_the_operation_error() {
    let (mut session, handle) = call(
        r#"printf '{"guid":"%s","status":"failed","contents":{"failure":"operation","error":{"refused":true}}}\n' "$guid""#,
    );
    let (events, result) = collect(handle).await;
    assert!(events.is_empty());
    assert!(matches!(result, Err(Failure::Operation(EchoError::Refused))));
    session.close().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_and_decoding_failures_never_become_operation_errors() {
    let (mut session, handle) = call(
        r#"printf '{"guid":"%s","status":"failed","contents":{"failure":"dispatch","error":{"kind":"unknown_operation","operation":"test.echo"}}}\n' "$guid""#,
    );
    let (_, result) = collect(handle).await;
    assert!(matches!(
        result,
        Err(Failure::Dispatch(DispatchError::UnknownOperation { .. }))
    ));
    session.close().unwrap();

    let (mut session, handle) = call(
        r#"printf '{"guid":"%s","status":"data","contents":"not an event"}\n' "$guid"
IFS= read -r cancel
printf '{"guid":"%s","status":"failed","contents":{"failure":"cancelled"}}\n' "$guid""#,
    );
    let (_, result) = collect(handle).await;
    assert!(matches!(
        result,
        Err(Failure::Dispatch(DispatchError::InvalidEvent { .. }))
    ));
    session.close().unwrap();
}

#[derive(Debug, thiserror::Error)]
enum EchoError {
    #[error("refused")]
    Refused,
}

struct Echo;

impl Operation for Echo {
    const ID: &'static str = "test.echo";
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(Wire::new(
        |input: u64| Ok(json!(input)),
        |value: Value| {
            value.as_u64().ok_or_else(|| DispatchError::InvalidInput {
                operation: Echo::ID.to_owned(),
                message: "expected an unsigned integer".to_owned(),
            })
        },
        Ok,
        |value| {
            value.as_object().cloned().map(Value::Object).ok_or_else(|| {
                DispatchError::InvalidEvent {
                    operation: Echo::ID.to_owned(),
                    message: "expected an event object".to_owned(),
                }
            })
        },
        Ok,
        Ok,
        |_| Ok(json!({"refused":true})),
        |value| {
            if value == json!({"refused":true}) {
                Ok(EchoError::Refused)
            } else {
                Err(DispatchError::InvalidError {
                    operation: Echo::ID.to_owned(),
                    message: "unknown error".to_owned(),
                })
            }
        },
    ));
    type Env = ();
    type Input = u64;
    type Event = Value;
    type Output = Value;
    type Error = EchoError;

    fn run(
        _input: u64,
        _cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Value, EchoError>> + Send {
        async { Ok(json!({})) }
    }
}

#[derive(Clone, Copy)]
struct Provider;

impl crate::operation::Provide<()> for Provider {
    fn provide(&self) {}
}

fn settings() -> Settings {
    Settings::new("test-build", LogLevel::Warn, std::sync::Arc::new(|_| {}))
}

/// Emits more events than the bounded queue holds, then reports how many it emitted.
struct Flood;

impl Operation for Flood {
    const ID: &'static str = "test.flood";
    const PLACEMENT: Placement = Placement::Host;
    type Env = ();
    type Input = u64;
    type Event = Value;
    type Output = Value;
    type Error = EchoError;

    fn run(
        total: u64,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Value, EchoError>> + Send {
        async move {
            for index in 0..total {
                cx.emit(json!({"index":index}))
                    .await
                    .map_err(|_| EchoError::Refused)?;
            }
            Ok(json!({"emitted": total}))
        }
    }
}

#[test]
fn settings_and_targets_are_reported_without_connecting() {
    let settings = Settings::new("test-build", LogLevel::Debug, std::sync::Arc::new(|_| {}));
    assert_eq!(settings.build(), "test-build");
    assert_eq!(settings.level(), LogLevel::Debug);
    let executor = Executor::new(Provider, settings);
    assert_eq!(executor.registry().ids(), Vec::<String>::new());
    assert!(!Target::Local.is_remote());
    assert!(
        Target::Remote(crate::ssh::Connection::new("ssh host", "mcpm-tools")).is_remote()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn surface_operations_never_open_a_connection() {
    let connection = crate::ssh::Connection::new("ssh host", "mcpm-tools");
    let session = Session::new(
        Executor::new(
            Provider,
            Settings::new("test-build", LogLevel::Warn, std::sync::Arc::new(|_| {})),
        ),
        Target::Remote(connection),
    );
    let mut handle = session.start::<Local>(()).await.unwrap();
    assert_eq!(handle.next().await, Some(json!({"local":true})));
    assert_eq!(handle.finish().await.unwrap(), json!({"local":true}));
}

struct Local;

impl Operation for Local {
    const ID: &'static str = "test.local";
    const PLACEMENT: Placement = Placement::Surface;
    type Env = ();
    type Input = ();
    type Event = Value;
    type Output = Value;
    type Error = EchoError;

    fn run(
        _input: (),
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Value, EchoError>> + Send {
        async move {
            cx.emit(json!({"local":true}))
                .await
                .expect("the caller reads events");
            Ok(json!({"local":true}))
        }
    }
}

#[test]
fn protocol_versions_are_compared_before_any_call() {
    let nil = Uuid::nil();
    let script = format!(
        "printf '%s\n' '{{\"guid\":\"{nil}\",\"status\":\"ready\",\"contents\":{{\"version\":5,\"build\":\"other\",\"operations\":[]}}}}'\ncat >/dev/null\n"
    );
    let mut command = ProcessCommand::new("sh");
    command.args(["-c", &script]);
    let error = session::Session::start(
        &mut command,
        session::Options::new("test-build", LogLevel::Warn, std::sync::Arc::new(|_| {})),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        crate::error::CallFailure::Dispatch(DispatchError::Protocol { .. })
    ));
}
/// A rejected handshake reaches the caller as a dispatch failure of its own call, never as a
/// transport failure: the requested call was refused, so retrying the connection cannot help.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn rejected_protocol_handshake_reaches_the_caller_as_dispatch_failure() {
    use std::os::unix::fs::PermissionsExt;

    let nil = Uuid::nil();
    let script = std::env::temp_dir().join(format!(
        "mcpm-exec-fake-ssh-{}-protocol",
        std::process::id()
    ));
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\n' '{{\"guid\":\"{nil}\",\"status\":\"ready\",\"contents\":{{\"version\":5,\"build\":\"test-build\",\"operations\":[]}}}}'\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let connection = crate::ssh::Connection::new(
        script.to_str().expect("temporary paths are UTF-8"),
        "mcpm-tools",
    );
    let session = Session::new(
        Executor::new(
            Provider,
            Settings::new("test-build", LogLevel::Warn, std::sync::Arc::new(|_| {})),
        ),
        Target::Remote(connection),
    );
    let error = session.start::<Echo>(1).await.err().unwrap();
    let message = format!("{error:?}");
    assert!(
        matches!(error, Failure::Dispatch(DispatchError::Protocol { .. })),
        "protocol mismatch behind a dispatch failure: {message}"
    );
    let _ = std::fs::remove_file(&script);
}
