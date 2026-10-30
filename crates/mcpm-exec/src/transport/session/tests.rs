//! Reply routing, request isolation, failure classification and handshake validation.

use std::{
    io::Cursor,
    process::Command as ProcessCommand,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
};

use serde_json::json;
use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};
use uuid::Uuid;

use super::*;

struct BuildWarningLayer {
    records: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl<S> Layer<S> for BuildWarningLayer
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
        if *event.metadata().level() != tracing::Level::WARN {
            return;
        }
        let mut visitor = BuildWarningVisitor::default();
        event.record(&mut visitor);
        if visitor.message == "remote build version differs" {
            self.records.lock().unwrap().push((
                visitor.message,
                visitor.local_build,
                visitor.remote_build,
            ));
        }
    }
}

#[derive(Default)]
struct BuildWarningVisitor {
    message: String,
    local_build: String,
    remote_build: String,
}

impl tracing::field::Visit for BuildWarningVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        match field.name() {
            "message" => self.message = value.trim_matches('"').to_owned(),
            "local_build" => self.local_build = value.trim_matches('"').to_owned(),
            "remote_build" => self.remote_build = value.trim_matches('"').to_owned(),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_owned(),
            "local_build" => self.local_build = value.to_owned(),
            "remote_build" => self.remote_build = value.to_owned(),
            _ => {}
        }
    }
}

fn options(build: &str) -> Options {
    Options::new(build, LogLevel::Warn, Arc::new(|_| {}))
}

/// One route with its bounded queue for tests that drive the reader directly.
fn route_pair(capacity: usize) -> (Route, Receiver<Result<Frame, CallFailure>>) {
    let (sender, receive) = sync_channel(capacity);
    (
        Route {
            sender,
            signal: Arc::new(Notify::new()),
        },
        receive,
    )
}

fn shared_with(routes: Routes) -> Shared {
    Shared {
        input: Mutex::new(Input { pipe: None }),
        routes: Mutex::new(routes),
    }
}

#[test]
fn interleaved_frames_and_abandoned_receivers() {
    let shared = shared_with(Routes {
        closing: true,
        ..Routes::new(Arc::new(|_| {}))
    });
    let (first_route, first) = route_pair(EVENT_CAPACITY);
    let (second_route, second) = route_pair(EVENT_CAPACITY);
    let a = Uuid::from_u128(1);
    let b = Uuid::from_u128(2);
    shared.routes.lock().unwrap().pending.insert(a, first_route);
    shared.routes.lock().unwrap().pending.insert(b, second_route);
    let frames = [
        json!({"guid":a,"status":"data","contents":{"whatever":42}}),
        json!({"guid":b,"status":"data","contents":["opaque",false]}),
        json!({"guid":b,"status":"finished","contents":null}),
        json!({"guid":a,"status":"finished","contents":null}),
        json!({"guid":Uuid::nil(),"status":"closed","contents":null}),
    ]
    .iter()
    .map(|value| format!("{value}\n"))
    .collect::<String>();
    drop(first);
    route(&mut Cursor::new(frames), &shared).unwrap();
    assert_eq!(
        second.recv().unwrap().unwrap().into_contents(),
        json!(["opaque", false])
    );
    assert_eq!(second.recv().unwrap().unwrap().status(), Status::Finished);
    assert!(shared.routes.lock().unwrap().closed);
    assert!(shared.routes.lock().unwrap().pending.is_empty());
}

#[test]
fn failed_frames_keep_operation_and_dispatch_failures_apart() {
    let shared = shared_with(Routes::new(Arc::new(|_| {})));
    let (entry, receive) = route_pair(EVENT_CAPACITY);
    let guid = Uuid::from_u128(9);
    shared.routes.lock().unwrap().pending.insert(guid, entry);
    let frames = format!(
        "{}\n",
        json!({
            "guid":guid,
            "status":"failed",
            "contents":{"failure":"operation","error":{"kind":"AlreadyRunning"}},
        })
    );
    let _ = route(&mut Cursor::new(frames), &shared);
    assert_eq!(
        receive.recv().unwrap().unwrap_err(),
        CallFailure::Operation(json!({"kind":"AlreadyRunning"}))
    );
    assert!(shared.routes.lock().unwrap().pending.is_empty());
}

/// The reader is the only consumer of the remote stream, so a full call queue must stop it:
/// that is where a slow caller throttles the remote host.
#[test]
fn reader_blocks_while_a_call_queue_is_full() {
    let shared = Arc::new(shared_with(Routes::new(Arc::new(|_| {}))));
    let guid = Uuid::from_u128(3);
    let (entry, receive) = route_pair(1);
    shared.routes.lock().unwrap().pending.insert(guid, entry);
    let finished = Arc::new(AtomicBool::new(false));
    let frames = [
        json!({"guid":guid,"status":"data","contents":{"index":0}}),
        json!({"guid":guid,"status":"data","contents":{"index":1}}),
        json!({"guid":guid,"status":"finished","contents":null}),
    ]
    .iter()
    .map(|value| format!("{value}\n"))
    .collect::<String>();
    let reader = {
        let shared = shared.clone();
        let finished = finished.clone();
        std::thread::spawn(move || {
            let _ = route(&mut Cursor::new(frames), &shared);
            finished.store(true, Ordering::Release);
        })
    };
    // The second frame cannot be delivered until the caller consumes the first one.
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        !finished.load(Ordering::Acquire),
        "the reader kept going while the call queue was full"
    );
    for expected in 0..2 {
        let frame = receive.recv().unwrap().unwrap();
        assert_eq!(frame.into_contents(), json!({"index":expected}));
    }
    assert_eq!(receive.recv().unwrap().unwrap().status(), Status::Finished);
    reader.join().unwrap();
    assert!(finished.load(Ordering::Acquire));
}

#[test]
fn log_frames_reach_the_session_sink_instead_of_a_call() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let shared = shared_with(Routes::new(Arc::new(move |record| {
        sink.lock().unwrap().push(record);
    })));
    let guid = Uuid::from_u128(4);
    let frame = Frame::log(
        guid,
        LogRecord::new(
            LogLevel::Info,
            "mcpm",
            std::collections::BTreeMap::from([("message".to_owned(), "hello".to_owned())]),
            Vec::new(),
        ),
    );
    route(
        &mut Cursor::new(format!("{}\n", serde_json::to_string(&frame).unwrap())),
        &shared,
    )
    .unwrap_err();
    let received = received.lock().unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].target(), "mcpm");
}

#[cfg(unix)]
#[test]
fn parallel_calls_route_replies_and_report_cancellation() {
    let build = "test-build";
    let script = format!(
        r#"
printf '%s\n' '{{"guid":"{nil}","status":"ready","contents":{{"version":{version},"build":"{build}","operations":["test.operation"]}}}}'
IFS= read -r first
IFS= read -r second
first_guid=$(printf '%s' "$first" | cut -d '"' -f4)
second_guid=$(printf '%s' "$second" | cut -d '"' -f4)
printf '{{"guid":"%s","status":"data","contents":["opaque",false]}}\n{{"guid":"%s","status":"finished","contents":null}}\n' "$second_guid" "$second_guid"
printf '{{"guid":"%s","status":"finished","contents":null}}\n' "$first_guid"
IFS= read -r third
IFS= read -r cancel
third_guid=$(printf '%s' "$third" | cut -d '"' -f4)
printf '{{"guid":"%s","status":"failed","contents":{{"failure":"cancelled"}}}}\n' "$third_guid"
cat >/dev/null
printf '%s\n' '{{"guid":"{nil}","status":"closed","contents":null}}'
"#,
        nil = Uuid::nil(),
        version = protocol::VERSION,
        build = build,
    );
    let mut command = ProcessCommand::new("sh");
    command.args(["-c", &script]);
    let mut session = Session::start(&mut command, options(build)).unwrap();
    assert_eq!(session.operations(), ["test.operation"]);
    assert_eq!(
        session.submit("test.unknown", json!({})).unwrap_err(),
        CallFailure::Dispatch(DispatchError::UnknownOperation {
            operation: "test.unknown".to_owned(),
        })
    );
    let mut first = session.submit("test.operation", json!({"input":1})).unwrap();
    let mut second = session.submit("test.operation", json!({"input":2})).unwrap();
    assert!(!first.guid().is_nil());
    assert_ne!(first.guid(), second.guid());
    assert_eq!(
        second.next_blocking().unwrap().into_contents(),
        json!(["opaque", false])
    );
    assert_eq!(second.next_blocking().unwrap().status(), Status::Finished);
    assert_eq!(first.next_blocking().unwrap().status(), Status::Finished);
    let mut third = session.submit("test.operation", json!({})).unwrap();
    third.cancel().unwrap();
    assert_eq!(
        third.next_blocking().unwrap_err(),
        CallFailure::Cancelled,
        "cancellation is a failure of the call, not of the session"
    );
    session.close().unwrap();
}

#[cfg(unix)]
#[test]
fn build_mismatch_session_can_submit_a_call() {
    let script = format!(
        r#"
printf '%s\n' '{{"guid":"{nil}","status":"ready","contents":{{"version":{version},"build":"other","operations":["test.operation"]}}}}'
IFS= read -r request
guid=$(printf '%s' "$request" | cut -d '"' -f4)
printf '{{"guid":"%s","status":"finished","contents":null}}\n' "$guid"
cat >/dev/null
printf '%s\n' '{{"guid":"{nil}","status":"closed","contents":null}}'
"#,
        nil = Uuid::nil(),
        version = protocol::VERSION,
    );
    let mut command = ProcessCommand::new("sh");
    command.args(["-c", &script]);
    let mut session = Session::start(&mut command, options("test-build")).unwrap();
    let mut call = session.submit("test.operation", json!({"input":1})).unwrap();
    assert_eq!(call.next_blocking().unwrap().status(), Status::Finished);
    session.close().unwrap();
}

#[cfg(unix)]
#[test]
fn handshake_allows_wrong_build_and_rejects_wrong_protocol_version() {
    let nil = Uuid::nil();
    let records = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(BuildWarningLayer {
        records: records.clone(),
    });
    let contents = format!(
        "{{\"version\":{},\"build\":\"other\",\"operations\":[]}}",
        protocol::VERSION
    );
    tracing::subscriber::with_default(subscriber, || {
        let frame = format!(
            "{{\"guid\":\"{nil}\",\"status\":\"ready\",\"contents\":{contents}}}\n"
        );
        let ready = handshake(&mut Cursor::new(frame), &options("test-build")).unwrap();
        assert!(ready.operations.is_empty());
    });
    assert_eq!(
        *records.lock().unwrap(),
        vec![(
            "remote build version differs".to_owned(),
            "test-build".to_owned(),
            "other".to_owned(),
        )]
    );

    let script = format!(
        "printf '%s\n' '{{\"guid\":\"{nil}\",\"status\":\"ready\",\"contents\":{{\"version\":5,\"build\":\"other\",\"operations\":[]}}}}'\ncat >/dev/null\n"
    );
    let mut command = ProcessCommand::new("sh");
    command.args(["-c", &script]);
    let error = Session::start(&mut command, options("test-build")).unwrap_err();
    assert!(matches!(
        error,
        CallFailure::Dispatch(DispatchError::Protocol { .. })
    ));
}
