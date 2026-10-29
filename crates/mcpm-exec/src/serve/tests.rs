//! Local dispatch, concurrency, cancellation, unknown identifiers, undecodable input,
//! panics, per-call log routing and EOF shutdown.

// Test operations mirror the trait's `impl Future` signature so their shape stays explicit.
#![allow(clippy::manual_async_fn)]

use std::{
    future::Future,
    io::{BufReader, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;
use uuid::Uuid;

use super::*;
use crate::{
    error::{DispatchError, Failure},
    executor::{Executor, Provide, Session, Settings, Target},
    operation::{Context, Operation, Placement, Wire},
    protocol::{LogLevel, LogRecord, Status},
    registry::Registry,
};

#[tokio::test(flavor = "multi_thread")]
async fn local_dispatch_streams_events_and_returns_the_output() {
    let session = Session::new(Executor::new(TestProvider, settings()), Target::Local);
    let mut handle = session.start::<Count>(3).await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = handle.next().await {
        events.push(event);
    }
    assert_eq!(handle.finish().await.unwrap(), json!({"count":3}));
    assert_eq!(
        events,
        [json!({"count":0}), json!({"count":1}), json!({"count":2})]
    );

    let handle = session.start::<Refuse>(()).await.unwrap();
    assert!(matches!(
        handle.finish().await,
        Err(Failure::Operation(TestError::Refused))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_a_handle_stops_the_operation() {
    let session = Session::new(Executor::new(TestProvider, settings()), Target::Local);
    let mut handle = session.start::<Stop>(()).await.unwrap();
    let first = handle.next().await.unwrap();
    drop(handle);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let observed = first["emitted"].as_u64().unwrap();
    assert!(
        STOPPED.load(Ordering::Acquire) <= observed + 1,
        "the operation kept running after its handle was dropped"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_runs_concurrent_calls_and_reports_every_failure_class() {
    let (client, server) = streams();
    let serve = tokio::spawn(serve_stream(server));
    let mut reader = BufReader::new(client.try_clone().unwrap());
    let writer = &client;

    let ready = read(&mut reader);
    assert_eq!(ready.status(), Status::Ready);
    let ready: crate::protocol::Ready = ready.decode().unwrap();
    assert_eq!(ready.version, protocol::VERSION);
    assert_eq!(ready.build, "test-build");
    assert_eq!(
        ready.operations,
        [Count::ID, Forever::ID, Panic::ID, Refuse::ID]
    );

    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    request(writer, first, Count::ID, json!(3));
    request(writer, second, Count::ID, json!(1));
    let mut first_events = 0;
    let mut second_events = 0;
    let mut finished = (false, false);
    while finished != (true, true) {
        let frame = read(&mut reader);
        match (frame.guid(), frame.status()) {
            (guid, Status::Data) if guid == first => first_events += 1,
            (guid, Status::Data) if guid == second => second_events += 1,
            (guid, Status::Finished) if guid == first => {
                assert_eq!(frame.decode::<Value>().unwrap(), json!({"count":3}));
                finished.0 = true;
            }
            (guid, Status::Finished) if guid == second => {
                assert_eq!(frame.decode::<Value>().unwrap(), json!({"count":1}));
                finished.1 = true;
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(first_events, 3);
    assert_eq!(second_events, 1);

    let unknown = Uuid::new_v4();
    request(writer, unknown, "test.missing", json!({}));
    assert_eq!(
        failure(&mut reader, unknown),
        crate::protocol::FailurePayload::Dispatch {
            error: DispatchError::UnknownOperation {
                operation: "test.missing".to_owned(),
            },
        }
    );

    let invalid = Uuid::new_v4();
    request(writer, invalid, Count::ID, json!({"count":"many"}));
    assert!(matches!(
        failure(&mut reader, invalid),
        crate::protocol::FailurePayload::Dispatch {
            error: DispatchError::InvalidInput { .. },
        }
    ));

    let panicked = Uuid::new_v4();
    request(writer, panicked, Panic::ID, Value::Null);
    let payload = failure(&mut reader, panicked);
    assert!(
        matches!(
            payload,
            crate::protocol::FailurePayload::Dispatch {
                error: DispatchError::Panicked { .. },
            }
        ),
        "{payload:?}"
    );

    let cancelled = Uuid::new_v4();
    request(writer, cancelled, Forever::ID, Value::Null);
    let mut started = false;
    while !started {
        let frame = read(&mut reader);
        assert_eq!(frame.guid(), cancelled);
        started = frame.status() == Status::Data;
    }
    protocol::write(
        &mut &*writer,
        &crate::protocol::Message::Cancel(crate::protocol::Cancel { cancel: cancelled }),
    )
    .unwrap();
    assert_eq!(
        failure(&mut reader, cancelled),
        crate::protocol::FailurePayload::Cancelled
    );

    let accepted = Uuid::new_v4();
    request(writer, accepted, Forever::ID, Value::Null);
    while read(&mut reader).status() != Status::Data {}
    writer.shutdown(Shutdown::Write).unwrap();
    loop {
        let frame = read(&mut reader);
        if frame.status() == Status::Closed {
            assert!(frame.guid().is_nil());
            break;
        }
        assert_eq!(frame.guid(), accepted);
    }
    serve.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn unusable_input_ends_the_session_without_a_close_frame() {
    let (client, server) = streams();
    let serve = tokio::spawn(serve_stream(server));
    let mut reader = BufReader::new(client.try_clone().unwrap());
    assert_eq!(read(&mut reader).status(), Status::Ready);
    writeln!(&client, "{{\"guid\":").unwrap();
    let frame = read(&mut reader);
    assert_eq!(frame.status(), Status::Failed);
    assert!(frame.guid().is_nil());
    let result = serve.await.unwrap();
    assert!(result.is_err(), "a protocol violation must not close normally");
}

#[test]
fn duplicate_and_non_remote_registrations_are_rejected() {
    let mut registry = Registry::new(TestProvider);
    registry.register::<Count>().unwrap();
    assert_eq!(
        registry.register::<Count>().unwrap_err(),
        crate::registry::RegistryError::DuplicateId { id: Count::ID }
    );
    assert_eq!(
        registry.register::<Surface>().unwrap_err(),
        crate::registry::RegistryError::NotRemotable { id: Surface::ID }
    );
    assert_eq!(registry.ids(), [Count::ID]);
}

#[test]
fn log_records_are_routed_to_the_call_that_emitted_them() {
    let (frames, mut received) = mpsc::channel(8);
    let guid = Uuid::new_v4();
    let _subscription = logs::subscribe(guid, LogLevel::Info, frames);
    let subscriber = tracing_subscriber::registry().with(logs::Capture);
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("operation", guid = %guid, operation = "test.count");
        let _entered = span.enter();
        tracing::debug!("below the requested level");
        tracing::info!(step = 1, "counted");
    });
    let frame = received.try_recv().unwrap();
    assert_eq!(frame.guid(), guid);
    assert_eq!(frame.status(), Status::Log);
    let mut record: LogRecord = frame.decode().unwrap();
    assert_eq!(record.level(), LogLevel::Info);
    assert_eq!(record.take_message(), "counted");
    assert_eq!(record.spans()[0].name(), "operation");
    assert!(received.try_recv().is_err());
}

fn settings() -> Settings {
    Settings::new("test-build", LogLevel::Warn, Arc::new(|_| {}))
}

async fn serve_stream(
    server: TcpStream,
) -> Result<(), crate::error::TransportError> {
    serve(
        BufReader::new(server.try_clone().unwrap()),
        server,
        registry(),
        "test-build",
    )
    .await
}

fn registry() -> Registry<TestProvider> {
    let mut registry = Registry::new(TestProvider);
    registry.register::<Count>().unwrap();
    registry.register::<Refuse>().unwrap();
    registry.register::<Panic>().unwrap();
    registry.register::<Forever>().unwrap();
    registry
}

fn streams() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}

fn read(reader: &mut impl std::io::BufRead) -> crate::protocol::Frame {
    protocol::read(reader).unwrap().unwrap()
}

fn request(
    writer: &TcpStream,
    guid: Uuid,
    operation: &str,
    input: Value,
) {
    protocol::write(
        &mut &*writer,
        &crate::protocol::Message::Request(crate::protocol::Request {
            guid,
            operation: operation.to_owned(),
            level: LogLevel::Warn,
            input,
        }),
    )
    .unwrap();
}

/// Reads frames until the call's terminal failure and returns its payload.
fn failure(
    reader: &mut impl std::io::BufRead,
    guid: Uuid,
) -> crate::protocol::FailurePayload {
    loop {
        let frame = read(reader);
        assert_eq!(frame.guid(), guid, "frame for another call: {frame:?}");
        if frame.status() == Status::Failed {
            return frame.decode().unwrap();
        }
        assert_eq!(frame.status(), Status::Data);
    }
}

#[derive(Clone, Copy)]
struct TestProvider;
impl Provide<()> for TestProvider {
    fn provide(&self) {}
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error("refused")]
    Refused,
}

fn count_input(value: Value) -> Result<u64, DispatchError> {
    value
        .get("count")
        .and_then(Value::as_u64)
        .or_else(|| value.as_u64())
        .ok_or_else(|| DispatchError::InvalidInput {
            operation: Count::ID.to_owned(),
            message: "expected an unsigned integer".to_owned(),
        })
}

/// Test-only encoder; the tests never observe an encoding failure.
fn encode<T: serde::Serialize>(value: T) -> Result<Value, DispatchError> {
    serde_json::to_value(value).map_err(|error| DispatchError::InvalidOutput {
        operation: "test".to_owned(),
        message: error.to_string(),
    })
}

/// Test-only decoder; the tests never observe a decoding failure.
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, DispatchError> {
    serde_json::from_value(value).map_err(|error| DispatchError::InvalidInput {
        operation: "test".to_owned(),
        message: error.to_string(),
    })
}

fn encode_message(error: TestError) -> Result<Value, DispatchError> {
    Ok(json!({"message": error.to_string()}))
}

fn decode_message(_value: Value) -> Result<TestError, DispatchError> {
    Ok(TestError::Refused)
}

struct Count;
impl Operation for Count {
    const ID: &'static str = "test.count";
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(Wire::new(
        encode::<u64>,
        count_input,
        encode::<Value>,
        decode::<Value>,
        encode::<Value>,
        decode::<Value>,
        encode_message,
        decode_message,
    ));
    type Env = ();
    type Input = u64;
    type Event = Value;
    type Output = Value;
    type Error = TestError;

    fn run(input: u64, cx: Context<'_, Self>) -> impl Future<Output = Result<Value, TestError>> + Send {
        async move {
            for index in 0..input {
                cx.emit(json!({"count":index}))
                    .await
                    .map_err(|_| TestError::Refused)?;
            }
            Ok(json!({"count":input}))
        }
    }
}

struct Refuse;
impl Operation for Refuse {
    const ID: &'static str = "test.refuse";
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(Wire::new(
        encode::<()>,
        decode::<()>,
        encode::<Value>,
        decode::<Value>,
        encode::<Value>,
        decode::<Value>,
        encode_message,
        decode_message,
    ));
    type Env = ();
    type Input = ();
    type Event = Value;
    type Output = Value;
    type Error = TestError;

    fn run(_input: (), _cx: Context<'_, Self>) -> impl Future<Output = Result<Value, TestError>> + Send {
        async { Err(TestError::Refused) }
    }
}

struct Panic;
impl Operation for Panic {
    const ID: &'static str = "test.panic";
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(Wire::new(
        encode::<()>,
        decode::<()>,
        encode::<Value>,
        decode::<Value>,
        encode::<Value>,
        decode::<Value>,
        encode_message,
        decode_message,
    ));
    type Env = ();
    type Input = ();
    type Event = Value;
    type Output = Value;
    type Error = TestError;

    fn run(_input: (), _cx: Context<'_, Self>) -> impl Future<Output = Result<Value, TestError>> + Send {
        async { panic!("operation panicked on purpose") }
    }
}

/// Emissions of the endless test operation, used to observe cancellation.
static EMITTED: AtomicU64 = AtomicU64::new(0);

struct Forever;
impl Operation for Forever {
    const ID: &'static str = "test.forever";
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(Wire::new(
        encode::<()>,
        decode::<()>,
        encode::<Value>,
        decode::<Value>,
        encode::<Value>,
        decode::<Value>,
        encode_message,
        decode_message,
    ));
    type Env = ();
    type Input = ();
    type Event = Value;
    type Output = Value;
    type Error = TestError;

    fn run(_input: (), cx: Context<'_, Self>) -> impl Future<Output = Result<Value, TestError>> + Send {
        async move {
            loop {
                let emitted = EMITTED.fetch_add(1, Ordering::AcqRel) + 1;
                cx.emit(json!({"emitted": emitted}))
                    .await
                    .map_err(|_| TestError::Refused)?;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
}

/// Emissions of the abandoned test operation, used to observe local cancellation.
static STOPPED: AtomicU64 = AtomicU64::new(0);

struct Stop;
impl Operation for Stop {
    const ID: &'static str = "test.stop";
    const PLACEMENT: Placement = Placement::Host;
    type Env = ();
    type Input = ();
    type Event = Value;
    type Output = Value;
    type Error = TestError;

    fn run(_input: (), cx: Context<'_, Self>) -> impl Future<Output = Result<Value, TestError>> + Send {
        async move {
            loop {
                let emitted = STOPPED.fetch_add(1, Ordering::AcqRel) + 1;
                cx.emit(json!({"emitted": emitted}))
                    .await
                    .map_err(|_| TestError::Refused)?;
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    }
}

struct Surface;
impl Operation for Surface {
    const ID: &'static str = "test.surface";
    const PLACEMENT: Placement = Placement::Surface;
    type Env = ();
    type Input = ();
    type Event = Value;
    type Output = ();
    type Error = TestError;

    fn run(_input: (), _cx: Context<'_, Self>) -> impl Future<Output = Result<(), TestError>> + Send {
        async { Ok(()) }
    }
}
