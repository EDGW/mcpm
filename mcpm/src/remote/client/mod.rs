//! Caller-owned process session with GUID routing, opaque contents and global-error decoding.

use std::collections::HashMap;
use std::io::BufReader;
use std::process::{Child, ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail, ensure};

use serde_json::Value;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use super::protocol::{self, Cancel, GlobalError, LogRequest, Logs, Ready, Reply, Request, Status};

mod traffic;

use traffic::{Metered, Traffic};

type Received = std::result::Result<Reply, Failure>;

#[derive(Debug, Clone)]
struct Failure {
    report: crate::diagnostics::ErrorReport,
    remote: bool,
}

impl Failure {
    fn error(&self) -> anyhow::Error {
        let error = self.report.clone().into_error();
        if self.remote {
            crate::diagnostics::RemoteError::wrap(error)
        } else {
            error
        }
    }
}

#[derive(Debug)]
pub(crate) struct Session {
    child: Child,
    shared: Arc<Shared>,
    reader: Option<JoinHandle<()>>,
    commands: Vec<String>,
    traffic: Traffic,
    traffic_reported: bool,
}

impl Session {
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    pub(crate) fn start(command: &mut ProcessCommand) -> Result<Self> {
        let started = Instant::now();
        info!(program = ?command.get_program(), "Starting remote session");
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to start remote session")?;
        debug!(
            pid = child.id(),
            "Remote process spawned; waiting for handshake"
        );
        let traffic = Traffic::default();
        let input = traffic.stdin(child.stdin.take().context("missing remote stdin")?);
        let mut output =
            BufReader::new(traffic.stdout(child.stdout.take().context("missing remote stdout")?));
        let shared = Arc::new(Shared {
            input: Mutex::new(Input { pipe: Some(input) }),
            routes: Mutex::new(Routes::default()),
        });
        let mut session = Self {
            child,
            shared,
            reader: None,
            commands: Vec::new(),
            traffic,
            traffic_reported: false,
        };
        let ready: Reply = protocol::read(&mut output)?.context("missing remote handshake")?;
        ensure!(
            ready.guid.is_nil() && ready.status == Status::Ready,
            "remote session did not send ready handshake"
        );
        let ready = ready.decode().context("invalid remote handshake")?;
        debug!("Received remote handshake: {:?}", ready);
        let Ready {
            version,
            commands,
            max_concurrency,
        } = ready;
        debug!(version, max_concurrency, "Verifying remote session");
        ensure!(
            version == protocol::VERSION && max_concurrency > 0,
            "unsupported remote session version/concurrency"
        );
        let log_guid = Uuid::new_v4();
        protocol::write(
            session.shared.input.lock().unwrap().pipe.as_mut().unwrap(),
            &Logs {
                logs: LogRequest {
                    guid: log_guid,
                    level: crate::logging::level(),
                },
            },
        )?;
        let ack: Reply = protocol::read(&mut output)?
            .context("missing remote log subscription acknowledgement")?;
        if ack.status == Status::Error {
            return Err(ack.decode::<GlobalError>()?.into_remote());
        }
        ensure!(
            ack.guid == log_guid && ack.status == Status::Ready && ack.contents.is_null(),
            "invalid remote log subscription acknowledgement"
        );
        session.shared.routes.lock().unwrap().logs = Some(LogRoute {
            guid: log_guid,
            finished: false,
        });
        debug!(%log_guid, level = ?crate::logging::level(), "Established remote log subscription");
        session.commands = commands;
        let shared = session.shared.clone();
        let span = tracing::Span::current();
        session.reader = Some(thread::spawn(move || {
            let _entered = span.enter();
            debug!("Remote response reader started");
            if let Err(error) = route(&mut output, &shared) {
                shared.fail(&error);
            }
            debug!("Remote response reader stopped");
        }));
        info!(
            commands = session.commands.len(),
            max_concurrency,
            elapsed_ms = started.elapsed().as_millis(),
            "Started remote session"
        );
        Ok(session)
    }

    /// Generate an invocation GUID and submit without waiting, routing replies to its handle.
    #[tracing::instrument(level = "debug", skip_all, fields(command), err(level = "debug"))]
    pub(crate) fn submit(&self, command: &str, contents: Value) -> Result<Pending> {
        ensure!(
            self.commands.iter().any(|id| id == command),
            "unsupported remote command ID: {command}"
        );
        let mut input = self.shared.input.lock().unwrap();
        ensure!(input.pipe.is_some(), "remote session is closed");
        let guid = Uuid::new_v4();
        info!(command, %guid, "Submitting remote command");
        let (send, receive) = mpsc::channel();
        {
            let mut routes = self.shared.routes.lock().unwrap();
            if let Some(error) = &routes.failure {
                return Err(error.error());
            }
            ensure!(
                !routes.closing && !routes.closed,
                "remote session is closed"
            );
            routes.pending.insert(guid, send);
            debug!(%guid, pending = routes.pending.len(), "Registered reply route");
        }
        if let Err(error) = protocol::write(
            input.pipe.as_mut().unwrap(),
            &Request {
                guid,
                command: command.to_owned(),
                contents,
            },
        ) {
            self.shared.fail(&error);
            return Err(error);
        }
        debug!(%guid, "Successfully submitted remote command");
        Ok(Pending {
            guid,
            receive,
            shared: self.shared.clone(),
            finished: false,
        })
    }

    /// EOF drains accepted commands before the server acknowledges shutdown.
    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    pub(crate) fn close(&mut self) -> Result<()> {
        let started = Instant::now();
        info!(
            pid = self.child.id(),
            "Closing remote session; draining accepted commands"
        );
        {
            let mut input = self.shared.input.lock().unwrap();
            self.shared.routes.lock().unwrap().closing = true;
            input.pipe.take();
        }
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| anyhow!("remote response reader panicked"))?;
        }
        let status = self
            .child
            .wait()
            .context("failed to wait for remote session")?;
        self.report_traffic();
        debug!(%status, elapsed_ms = started.elapsed().as_millis(), "Remote process exited");
        ensure!(status.success(), "remote session exited with {status}");
        let routes = self.shared.routes.lock().unwrap();
        if let Some(error) = &routes.failure {
            return Err(error.error());
        }
        ensure!(routes.closed, "remote session did not acknowledge shutdown");
        info!("Remote session closed successfully");
        Ok(())
    }

    fn report_traffic(&mut self) {
        if !self.traffic_reported {
            let (stdin_bytes, stdout_bytes) = self.traffic.totals();
            info!(
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
            debug!(
                pid = self.child.id(),
                "Terminating remote process during session cleanup"
            );
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.report_traffic();
    }
}

#[derive(Debug)]
pub(crate) struct Pending {
    guid: Uuid,
    receive: mpsc::Receiver<Received>,
    shared: Arc<Shared>,
    finished: bool,
}

impl Pending {
    pub(crate) fn cancel(&self) -> Result<()> {
        debug!(guid = %self.guid, "Sending remote command cancellation");
        let mut input = self.shared.input.lock().unwrap();
        protocol::write(
            input.pipe.as_mut().context("remote session is closed")?,
            &Cancel { cancel: self.guid },
        )
    }

    /// Global statuses/errors are decoded here; Data contents belong to the command.
    #[tracing::instrument(level = "debug", skip_all, fields(guid = %self.guid), err(level = "debug"))]
    pub(crate) fn collect(mut self, mut receive: impl FnMut(Reply) -> Result<()>) -> Result<()> {
        let started = Instant::now();
        let mut messages = 0usize;
        loop {
            let reply = self
                .receive
                .recv()
                .context("remote response channel closed")?
                .map_err(|failure| failure.error())?;
            match reply.status {
                Status::Finished => {
                    self.finished = true;
                    ensure!(
                        reply.contents.is_null(),
                        "invalid remote completion contents"
                    );
                    info!(
                        messages,
                        elapsed_ms = started.elapsed().as_millis(),
                        "Remote command finished"
                    );
                    return Ok(());
                }
                Status::Error => {
                    self.finished = true;
                    let error = reply
                        .decode::<GlobalError>()
                        .context("invalid global remote error")?;
                    debug!(
                        ?error,
                        messages,
                        elapsed_ms = started.elapsed().as_millis(),
                        "Remote command failed"
                    );
                    return Err(error.into_remote());
                }
                Status::Data => {
                    messages += 1;
                    receive(reply)?;
                }
                _ => bail!("unexpected remote reply status"),
            }
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.finished {
            debug!(guid = %self.guid, "Cancelling abandoned remote command");
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
    fn fail(&self, error: &anyhow::Error) {
        let message = Failure {
            report: crate::diagnostics::ErrorReport::capture(error),
            remote: error
                .chain()
                .any(|cause| cause.is::<crate::diagnostics::RemoteError>()),
        };
        let mut routes = self.routes.lock().unwrap();
        warn!(error = %message.report.message, pending = routes.pending.len(), "Remote connection failed; notifying pending commands");
        routes.failure = Some(message.clone());
        for (_, send) in routes.pending.drain() {
            let _ = send.send(Err(message.clone()));
        }
    }
}

#[derive(Debug)]
struct Input {
    pipe: Option<Metered<ChildStdin>>,
}

#[derive(Default, Debug)]
struct Routes {
    logs: Option<LogRoute>,
    pending: HashMap<Uuid, mpsc::Sender<Received>>,
    closing: bool,
    closed: bool,
    failure: Option<Failure>,
}

#[derive(Debug)]
struct LogRoute {
    guid: Uuid,
    finished: bool,
}

fn route(reader: &mut impl std::io::BufRead, shared: &Shared) -> Result<()> {
    while let Some(reply) = protocol::read::<Reply>(reader)? {
        trace!(guid = %reply.guid, status = ?reply.status, "Received remote reply");
        if reply.guid.is_nil() {
            // Session Error
            if reply.status == Status::Error {
                let global_error = reply
                    .decode::<GlobalError>()
                    .context("invalid session error")?;
                warn!("Received session error: {:?}", global_error);
                return Err(global_error.into_remote());
            }
            // Session Close
            ensure!(
                reply.status == Status::Closed && reply.contents.is_null(),
                "unexpected remote session status"
            );
            debug!("Received session close acknowledgement");
            let mut routes = shared.routes.lock().unwrap();
            ensure!(
                routes.closing
                    && routes.pending.is_empty()
                    && routes.logs.as_ref().is_none_or(|logs| logs.finished),
                "unexpected remote session shutdown"
            );
            routes.closed = true;
            drop(routes);
            ensure!(
                protocol::read::<Reply>(reader)?.is_none(),
                "unexpected output after remote session shutdown"
            );
            return Ok(());
        }
        {
            let mut routes = shared.routes.lock().unwrap();
            if let Some(logs) = &mut routes.logs
                && logs.guid == reply.guid
            {
                ensure!(!logs.finished, "remote log reply after completion");
                match reply.status {
                    Status::Data => {
                        let record = reply.decode::<crate::logging::stream::Record>()?;
                        drop(routes);
                        record.replay();
                    }
                    Status::Finished => {
                        ensure!(
                            reply.contents.is_null(),
                            "invalid remote log completion contents"
                        );
                        logs.finished = true;
                    }
                    Status::Error => return Err(reply.decode::<GlobalError>()?.into_remote()),
                    _ => bail!("unexpected remote log status"),
                }
                continue;
            }
        }
        ensure!(
            matches!(
                reply.status,
                Status::Data | Status::Finished | Status::Error
            ),
            "unexpected request status"
        );
        let mut routes = shared.routes.lock().unwrap();
        let send = routes
            .pending
            .get(&reply.guid)
            .context("remote reply does not belong to an active request")?;
        let guid = reply.guid;
        let terminal = reply.status.is_terminal();
        // Discarding a handle leaves routing in place until its terminal reply.
        trace!(%guid, terminal, "Routing remote reply");
        if send.send(Ok(reply)).is_err() {
            trace!(%guid, "Discarding reply for abandoned command");
        }
        if terminal {
            debug!(%guid, "Removing completed command route");
            routes.pending.remove(&guid);
        }
    }
    bail!("remote session ended before shutdown acknowledgement")
}

#[cfg(test)]
mod tests;
