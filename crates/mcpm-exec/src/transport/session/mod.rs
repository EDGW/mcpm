//! Caller-side protocol session: one child process, GUID routing and per-call cancellation.
//!
//! The session owns the protocol pipe, one reader thread and the routes of every call in
//! flight. Calls submit encoded JSON and receive frames until their terminal frame; logs
//! are handed to the caller's sink instead of a call route.

use std::{
    collections::HashMap,
    io::BufReader,
    process::{Child, ChildStdin, Command as ProcessCommand, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use tokio::sync::Notify;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use super::super::{
    EVENT_CAPACITY,
    error::{CallFailure, DispatchError, TransportError},
    protocol::{
        self, Cancel, FailurePayload, Frame, LogLevel, LogRecord, Message, Ready, Request, Status,
    },
};

mod traffic;

use traffic::{Metered, Traffic};

/// Session settings supplied by the process entry point.
#[derive(Clone)]
pub struct Options {
    build: String,
    level: LogLevel,
    logs: Arc<dyn Fn(LogRecord) + Send + Sync>,
}

impl Options {
    /// `logs` receives every record the remote serve process forwards for this session.
    pub fn new(
        build: impl Into<String>,
        level: LogLevel,
        logs: Arc<dyn Fn(LogRecord) + Send + Sync>,
    ) -> Self {
        Self {
            build: build.into(),
            level,
            logs,
        }
    }
}

impl std::fmt::Debug for Options {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Options")
            .field("build", &self.build)
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

/// One protocol session with the process it started.
#[derive(Debug)]
pub struct Session {
    child: Child,
    shared: Arc<Shared>,
    reader: Option<JoinHandle<()>>,
    operations: Vec<String>,
    level: LogLevel,
    traffic: Traffic,
    traffic_reported: bool,
}

impl Session {
    /// Starts the process and completes the handshake; no log subscription is negotiated.
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    pub fn start(command: &mut ProcessCommand, options: Options) -> Result<Self, CallFailure> {
        let started = Instant::now();
        info!(program = ?command.get_program(), "Starting remote session");
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                CallFailure::Transport(TransportError::Start {
                    message: format!("failed to start the remote session: {error}"),
                })
            })?;
        debug!(pid = child.id(), "Remote process spawned; waiting for handshake");
        let traffic = Traffic::default();
        let input = traffic.stdin(child.stdin.take().ok_or_else(|| io_failure("missing remote stdin"))?);
        let mut output =
            BufReader::new(traffic.stdout(child.stdout.take().ok_or_else(|| io_failure("missing remote stdout"))?));
        let shared = Arc::new(Shared {
            input: Mutex::new(Input { pipe: Some(input) }),
            routes: Mutex::new(Routes::new(options.logs.clone())),
        });
        let mut session = Self {
            child,
            shared,
            reader: None,
            operations: Vec::new(),
            level: options.level,
            traffic,
            traffic_reported: false,
        };
        let ready = handshake(&mut output, &options)?;
        debug!(operations = ready.operations.len(), "Verified remote session");
        session.operations = ready.operations;
        let shared = session.shared.clone();
        let span = tracing::Span::current();
        session.reader = Some(thread::spawn(move || {
            let _entered = span.enter();
            debug!("Remote response reader started");
            if let Err(failure) = route(&mut output, &shared) {
                shared.fail(failure);
            }
            debug!("Remote response reader stopped");
        }));
        debug!(
            operations = session.operations.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "Started remote session"
        );
        Ok(session)
    }

    /// Whether the session can still accept calls.
    pub fn is_open(&self) -> bool {
        let open = self.shared.input.lock().unwrap().pipe.is_some();
        let routes = self.shared.routes.lock().unwrap();
        open && routes.failure.is_none() && !routes.closing && !routes.closed
    }

    /// Operation identifiers announced by the remote serve process.
    pub fn operations(&self) -> &[String] {
        &self.operations
    }

    /// Allocates a GUID and submits one encoded call without waiting for its result.
    #[tracing::instrument(level = "debug", skip_all, fields(operation), err(level = "debug"))]
    pub fn submit(&self, operation: &str, input: serde_json::Value) -> Result<Pending, CallFailure> {
        if !self.operations.iter().any(|id| id == operation) {
            return Err(CallFailure::Dispatch(DispatchError::UnknownOperation {
                operation: operation.to_owned(),
            }));
        }
        let mut pipe = self.shared.input.lock().unwrap();
        if pipe.pipe.is_none() {
            return Err(closed_failure());
        }
        let guid = Uuid::new_v4();
        info!(operation, %guid, "Submitting remote call");
        // One bounded queue per call: the reader blocks when the caller is behind, so a slow
        // consumer throttles the remote stream instead of growing memory.
        let (send, receive) = sync_channel(EVENT_CAPACITY);
        let signal = Arc::new(Notify::new());
        {
            let mut routes = self.shared.routes.lock().unwrap();
            if let Some(failure) = &routes.failure {
                return Err(failure.clone());
            }
            if routes.closing || routes.closed {
                return Err(closed_failure());
            }
            routes.pending.insert(
                guid,
                Route {
                    sender: send,
                    signal: signal.clone(),
                },
            );
            debug!(%guid, pending = routes.pending.len(), "Registered reply route");
        }
        let submitted = protocol::write(
            pipe.pipe.as_mut().expect("checked above"),
            &Message::Request(Request {
                guid,
                operation: operation.to_owned(),
                level: self.level,
                input,
            }),
        );
        if let Err(error) = submitted {
            let failure = CallFailure::Transport(TransportError::Io {
                message: format!("failed to write the remote request: {error}"),
            });
            self.shared.fail(failure.clone());
            return Err(failure);
        }
        debug!(%guid, "Submitted remote call");
        Ok(Pending {
            guid,
            receive,
            signal,
            shared: self.shared.clone(),
            finished: false,
            started: Instant::now(),
            messages: 0,
        })
    }

    /// EOF drains accepted calls before the server acknowledges shutdown.
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    pub fn close(&mut self) -> Result<(), CallFailure> {
        let started = Instant::now();
        debug!(pid = self.child.id(), "Closing remote session; draining accepted calls");
        {
            let mut input = self.shared.input.lock().unwrap();
            self.shared.routes.lock().unwrap().closing = true;
            input.pipe.take();
        }
        if let Some(reader) = self.reader.take()
            && reader.join().is_err()
        {
            return Err(io_failure("the remote response reader panicked"));
        }
        let status = self.child.wait().map_err(|error| {
            CallFailure::Transport(TransportError::Exited {
                message: format!("failed to wait for the remote session: {error}"),
            })
        })?;
        self.report_traffic();
        debug!(%status, elapsed_ms = started.elapsed().as_millis(), "Remote process exited");
        if !status.success() {
            return Err(CallFailure::Transport(TransportError::Exited {
                message: status.to_string(),
            }));
        }
        let routes = self.shared.routes.lock().unwrap();
        if let Some(failure) = &routes.failure {
            return Err(failure.clone());
        }
        if !routes.closed {
            return Err(io_failure("the remote session did not acknowledge shutdown"));
        }
        debug!("Remote session closed successfully");
        Ok(())
    }

    fn report_traffic(&mut self) {
        if !self.traffic_reported {
            let (stdin_bytes, stdout_bytes) = self.traffic.totals();
            debug!(
                stdin_bytes,
                stdout_bytes,
                total_bytes = stdin_bytes.saturating_add(stdout_bytes),
                "Remote session data transferred"
            );
            self.traffic_reported = true;
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.input.lock().unwrap().pipe.take();
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            debug!(pid = self.child.id(), "Terminating remote process during session cleanup");
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.report_traffic();
    }
}

/// Reads and validates the handshake before any call is routed.
fn handshake(reader: &mut impl std::io::BufRead, options: &Options) -> Result<Ready, CallFailure> {
    let frame = protocol::read::<Frame>(reader)
        .map_err(io_failure)?
        .ok_or_else(|| io_failure("missing remote handshake"))?;
    if !frame.guid().is_nil() || frame.status() != Status::Ready {
        return Err(protocol_failure(
            "the remote session did not send a ready handshake",
        ));
    }
    let ready: Ready = frame
        .decode()
        .map_err(|error| protocol_failure(format!("invalid remote handshake: {error}")))?;
    if ready.version != protocol::VERSION {
        return Err(protocol_failure(format!(
            "unsupported remote protocol version {}; expected {}",
            ready.version,
            protocol::VERSION
        )));
    }
    if ready.build != options.build {
        warn!(
            local_build = %options.build,
            remote_build = %ready.build,
            "remote build version differs"
        );
    }
    Ok(ready)
}

/// One call in flight; dropping it cancels the call.
#[derive(Debug)]
pub struct Pending {
    guid: Uuid,
    receive: Receiver<Result<Frame, CallFailure>>,
    signal: Arc<Notify>,
    shared: Arc<Shared>,
    finished: bool,
    started: Instant,
    messages: usize,
}

impl Pending {
    pub fn guid(&self) -> Uuid {
        self.guid
    }

    /// Asks the executor to stop; the caller stops reading after this.
    pub fn cancel(&self) -> Result<(), CallFailure> {
        debug!(guid = %self.guid, "Sending remote call cancellation");
        let mut input = self.shared.input.lock().unwrap();
        let pipe = input.pipe.as_mut().ok_or_else(closed_failure)?;
        protocol::write(pipe, &Message::Cancel(Cancel { cancel: self.guid })).map_err(|error| {
            CallFailure::Transport(TransportError::Io {
                message: format!("failed to write the remote cancellation: {error}"),
            })
        })
    }

    /// The next frame of this call, blocking the current thread.
    pub fn next_blocking(&mut self) -> Result<Frame, CallFailure> {
        let received = match self.receive.recv() {
            Ok(received) => received,
            Err(_) => {
                self.finished = true;
                return Err(self.closed_failure());
            }
        };
        self.decode(received)
    }

    /// The next frame of this call, without blocking the runtime.
    pub async fn next(&mut self) -> Result<Frame, CallFailure> {
        let received = loop {
            let signal = self.signal.notified();
            match self.receive.try_recv() {
                Ok(received) => break received,
                Err(TryRecvError::Empty) => signal.await,
                Err(TryRecvError::Disconnected) => {
                    self.finished = true;
                    return Err(self.closed_failure());
                }
            }
        };
        self.decode(received)
    }

    /// The session-level failure when the connection ended, so a caller learns why.
    fn closed_failure(&self) -> CallFailure {
        let routes = self.shared.routes.lock().unwrap();
        routes.failure.clone().unwrap_or_else(closed_failure)
    }

    fn decode(&mut self, received: Result<Frame, CallFailure>) -> Result<Frame, CallFailure> {
        let frame = match received {
            Ok(frame) => frame,
            Err(failure) => {
                self.finished = true;
                return Err(failure);
            }
        };
        match frame.status() {
            Status::Finished => {
                self.finished = true;
                debug!(
                    guid = %self.guid,
                    messages = self.messages,
                    elapsed_ms = self.started.elapsed().as_millis(),
                    "Remote call finished"
                );
            }
            Status::Data => self.messages += 1,
            status => {
                self.finished = true;
                return Err(protocol_failure(format!(
                    "unexpected reply frame for a call: {status:?}"
                )));
            }
        }
        Ok(frame)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.finished {
            debug!(guid = %self.guid, "Cancelling abandoned remote call");
            let _ = self.cancel();
        }
    }
}

#[derive(Debug)]
struct Shared {
    input: Mutex<Input>,
    routes: Mutex<Routes>,
}

impl Shared {
    fn fail(&self, failure: CallFailure) {
        let mut routes = self.routes.lock().unwrap();
        warn!(
            error = %format_args!("{failure:?}"),
            pending = routes.pending.len(),
            "Remote connection failed; notifying pending calls"
        );
        routes.failure = Some(failure);
        // Dropping a route closes its queue; the signal wakes a caller waiting on it, which
        // then reports the stored session failure.
        for (_, route) in routes.pending.drain() {
            route.signal.notify_one();
        }
    }
}

#[derive(Debug)]
struct Input {
    pipe: Option<Metered<ChildStdin>>,
}

struct Routes {
    logs: Arc<dyn Fn(LogRecord) + Send + Sync>,
    pending: HashMap<Uuid, Route>,
    closing: bool,
    closed: bool,
    failure: Option<CallFailure>,
}

/// One call's bounded reply queue plus the wakeup its asynchronous caller waits on.
struct Route {
    sender: SyncSender<Result<Frame, CallFailure>>,
    signal: Arc<Notify>,
}

impl std::fmt::Debug for Route {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Route").finish_non_exhaustive()
    }
}

impl Routes {
    fn new(logs: Arc<dyn Fn(LogRecord) + Send + Sync>) -> Self {
        Self {
            logs,
            pending: HashMap::new(),
            closing: false,
            closed: false,
            failure: None,
        }
    }
}

impl std::fmt::Debug for Routes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Routes")
            .field("pending", &self.pending.len())
            .field("closing", &self.closing)
            .field("closed", &self.closed)
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}

fn route(reader: &mut impl std::io::BufRead, shared: &Shared) -> Result<(), CallFailure> {
    while let Some(frame) = protocol::read::<Frame>(reader).map_err(io_failure)? {
        trace!(guid = %frame.guid(), status = ?frame.status(), "Received remote frame");
        if frame.guid().is_nil() {
            match frame.status() {
                Status::Failed => {
                    let failure = frame
                        .decode::<FailurePayload>()
                        .map_err(|error| protocol_failure(format!("invalid session error: {error}")))?;
                    warn!(?failure, "Received session failure");
                    return Err(payload_failure(failure));
                }
                Status::Closed => {
                    let routes = shared.routes.lock().unwrap();
                    if !(routes.closing && routes.pending.is_empty()) {
                        return Err(protocol_failure("unexpected remote session shutdown"));
                    }
                    drop(routes);
                    if protocol::read::<Frame>(reader).map_err(io_failure)?.is_some() {
                        return Err(protocol_failure(
                            "unexpected output after remote session shutdown",
                        ));
                    }
                    shared.routes.lock().unwrap().closed = true;
                    debug!("Received session close acknowledgement");
                    return Ok(());
                }
                status => {
                    return Err(protocol_failure(format!(
                        "unexpected session frame: {status:?}"
                    )));
                }
            }
        }
        if frame.status() == Status::Log {
            let record = frame
                .decode::<LogRecord>()
                .map_err(|error| protocol_failure(format!("invalid remote log record: {error}")))?;
            let logs = shared.routes.lock().unwrap().logs.clone();
            logs(record);
            continue;
        }
        let terminal = frame.status() == Status::Finished;
        let failure = match frame.status() {
            Status::Failed => Some(frame.decode::<FailurePayload>().map_err(|error| {
                protocol_failure(format!("invalid remote call failure: {error}"))
            })?),
            Status::Data | Status::Finished => None,
            status => {
                return Err(protocol_failure(format!(
                    "unexpected call frame: {status:?}"
                )));
            }
        };
        let guid = frame.guid();
        // Take the route out of the lock before sending: the queue is bounded, so this send
        // may block the reader until the caller reads or finishes.
        let (sender, signal) = {
            let mut routes = shared.routes.lock().unwrap();
            let route = routes
                .pending
                .get(&guid)
                .ok_or_else(|| protocol_failure("reply does not belong to an active call"))?;
            let sender = route.sender.clone();
            let signal = route.signal.clone();
            if terminal || failure.is_some() {
                routes.pending.remove(&guid);
            }
            (sender, signal)
        };
        let delivered = match failure {
            Some(failure) => sender.send(Err(payload_failure(failure))),
            None => sender.send(Ok(frame)),
        };
        if delivered.is_err() {
            trace!(%guid, "Discarding reply for an abandoned call");
        }
        signal.notify_one();
    }
    Err(io_failure(
        "the remote session ended before its shutdown acknowledgement",
    ))
}

fn payload_failure(payload: FailurePayload) -> CallFailure {
    match payload {
        FailurePayload::Operation { error } => CallFailure::Operation(error),
        FailurePayload::Dispatch { error } => CallFailure::Dispatch(error),
        FailurePayload::Cancelled => CallFailure::Cancelled,
    }
}

fn io_failure(message: impl std::fmt::Display) -> CallFailure {
    CallFailure::Transport(TransportError::Io {
        message: message.to_string(),
    })
}

fn closed_failure() -> CallFailure {
    CallFailure::Transport(TransportError::Closed {
        message: "the remote session is closed".to_owned(),
    })
}

fn protocol_failure(message: impl Into<String>) -> CallFailure {
    CallFailure::Dispatch(DispatchError::Protocol {
        message: message.into(),
    })
}

#[cfg(test)]
mod tests;
