//! Caller-owned process session with GUID routing, opaque contents and global-error decoding.

use std::collections::HashMap;
use std::io::BufReader;
use std::process::{Child, ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, anyhow, bail, ensure};

use serde_json::Value;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::protocol::{self, Cancel, GlobalError, Ready, Reply, Request, Status};

type Received = std::result::Result<Reply, String>;

#[derive(Debug)]
pub(crate) struct Session {
    child: Child,
    shared: Arc<Shared>,
    reader: Option<JoinHandle<()>>,
    commands: Vec<String>,
}

impl Session {
    pub(crate) fn start(command: &mut ProcessCommand) -> Result<Self> {
        info!("Starting remote session: {:?}", command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to start remote session")?;
        let input = child.stdin.take().context("missing remote stdin")?;
        let mut output = BufReader::new(child.stdout.take().context("missing remote stdout")?);
        let shared = Arc::new(Shared {
            input: Mutex::new(Input { pipe: Some(input) }),
            routes: Mutex::new(Routes::default()),
        });
        let mut session = Self {
            child,
            shared,
            reader: None,
            commands: Vec::new(),
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
        debug!("verifying remote session...");
        ensure!(
            version == protocol::VERSION && max_concurrency > 0,
            "unsupported remote session version/concurrency"
        );
        session.commands = commands;
        let shared = session.shared.clone();
        session.reader = Some(thread::spawn(move || {
            if let Err(error) = route(&mut output, &shared) {
                shared.fail(format!("{error:#}"));
            }
        }));
        info!(
            "Started remote session with {} commands and max concurrency {}",
            session.commands.len(),
            max_concurrency
        );
        Ok(session)
    }

    /// Generate an invocation GUID and submit without waiting, routing replies to its handle.
    pub(crate) fn submit(&self, command: &str, contents: Value) -> Result<Pending> {
        ensure!(
            self.commands.iter().any(|id| id == command),
            "unsupported remote command ID: {command}"
        );
        let mut input = self.shared.input.lock().unwrap();
        ensure!(input.pipe.is_some(), "remote session is closed");
        let guid = Uuid::new_v4();
        info!(
            "Submitting remote command {command} with guid {guid} and contents: {:?}",
            contents
        );
        let (send, receive) = mpsc::channel();
        {
            let mut routes = self.shared.routes.lock().unwrap();
            if let Some(error) = &routes.failure {
                bail!("{error}");
            }
            ensure!(
                !routes.closing && !routes.closed,
                "remote session is closed"
            );
            routes.pending.insert(guid, send);
        }
        if let Err(error) = protocol::write(
            input.pipe.as_mut().unwrap(),
            &Request {
                guid,
                command: command.to_owned(),
                contents,
            },
        ) {
            self.shared.fail(format!("{error:#}"));
            return Err(error);
        }
        debug!("Succesfully submitted command {guid}.");
        Ok(Pending {
            guid,
            receive,
            shared: self.shared.clone(),
            finished: false,
        })
    }

    /// EOF drains accepted commands before the server acknowledges shutdown.
    pub(crate) fn close(&mut self) -> Result<()> {
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
        ensure!(status.success(), "remote session exited with {status}");
        let routes = self.shared.routes.lock().unwrap();
        if let Some(error) = &routes.failure {
            bail!("{error}");
        }
        ensure!(routes.closed, "remote session did not acknowledge shutdown");
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.input.lock().unwrap().pipe.take();
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
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
        let mut input = self.shared.input.lock().unwrap();
        protocol::write(
            input.pipe.as_mut().context("remote session is closed")?,
            &Cancel { cancel: self.guid },
        )
    }

    /// Global statuses/errors are decoded here; Data contents belong to the command.
    pub(crate) fn collect(mut self, mut receive: impl FnMut(Reply) -> Result<()>) -> Result<()> {
        loop {
            let reply = self
                .receive
                .recv()
                .context("remote response channel closed")?
                .map_err(|message| anyhow!(message))?;
            match reply.status {
                Status::Finished => {
                    self.finished = true;
                    ensure!(
                        reply.contents.is_null(),
                        "invalid remote completion contents"
                    );
                    return Ok(());
                }
                Status::Error => {
                    self.finished = true;
                    return Err(reply
                        .decode::<GlobalError>()
                        .context("invalid global remote error")?
                        .into());
                }
                Status::Data => receive(reply)?,
                _ => bail!("unexpected remote reply status"),
            }
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.finished {
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
    fn fail(&self, message: String) {
        let mut routes = self.routes.lock().unwrap();
        routes.failure = Some(message.clone());
        for (_, send) in routes.pending.drain() {
            let _ = send.send(Err(message.clone()));
        }
    }
}

#[derive(Debug)]
struct Input {
    pipe: Option<ChildStdin>,
}

#[derive(Default, Debug)]
struct Routes {
    pending: HashMap<Uuid, mpsc::Sender<Received>>,
    closing: bool,
    closed: bool,
    failure: Option<String>,
}

fn route(reader: &mut impl std::io::BufRead, shared: &Shared) -> Result<()> {
    while let Some(reply) = protocol::read::<Reply>(reader)? {
        debug!(
            "Received reply with guid = {:?}, status = {:?}",
            reply.guid, reply.status
        );
        if reply.guid.is_nil() {
            // Session Error
            if reply.status == Status::Error {
                let global_error = reply
                    .decode::<GlobalError>()
                    .context("invalid session error")?;
                warn!("Received session error: {:?}", global_error);
                return Err(global_error.into());
            }
            // Session Close
            ensure!(
                reply.status == Status::Closed && reply.contents.is_null(),
                "unexpected remote session status"
            );
            debug!("Received session close acknowledgement");
            let mut routes = shared.routes.lock().unwrap();
            ensure!(
                routes.closing && routes.pending.is_empty(),
                "unexpected remote session shutdown"
            );
            routes.closed = true;
            drop(routes);
            ensure!(
                protocol::read::<Reply>(reader)?.is_none(),
                "unexpected output after remote session shutdown"
            );
            info!("Remote session closed successfully");
            return Ok(());
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
        debug!(
            "Routing reply with guid = {:?}, terminal = {}",
            guid, terminal
        );
        let _ = send.send(Ok(reply));
        if terminal {
            info!("Terminating request with guid = {:?}", guid);
            routes.pending.remove(&guid);
        }
    }
    bail!("remote session ended before shutdown acknowledgement")
}

#[cfg(test)]
mod tests;
