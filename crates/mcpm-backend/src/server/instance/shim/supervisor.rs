//! Shim supervisor: owns the runtime lock, Java stdio, state and socket lifecycle.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use mcpm_exec::ipc::{ExclusiveLock, LocalStream, SocketAddress};

use crate::data::server::instance::{
    paths::InstancePaths,
    protocol::{self, Frame},
    state::{self, ExitRecord, InstanceState, InstanceStatus},
};

use super::{input, output::OutputBuffer, process};

/// Immutable launch data transferred through the detached startup pipe.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StartSpec {
    pub executable: PathBuf,
    pub directory: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub arguments: Vec<String>,
    pub argument_file: bool,
    pub server_root: PathBuf,
    pub instance_id: String,
    pub plan: String,
    pub stop_timeout_secs: u64,
}

/// Runs a shim and reports startup only after lock, socket, Java and state are ready.
pub fn run(spec: StartSpec) -> Result<()> {
    run_with_handshake(spec, |_| Ok(()))
}

/// Runs a shim, invoking `started` after all startup invariants are established.
/// If setup fails, the callback is never called and the error is returned to the entrypoint.
pub fn run_with_handshake(
    spec: StartSpec,
    started: impl FnOnce(&InstanceState) -> Result<()>,
) -> Result<()> {
    let (runtime, initial) = Runtime::prepare(&spec)?;
    started(&initial)?;
    runtime.supervise()
}

struct Runtime {
    paths: InstancePaths,
    socket: SocketAddress,
    _lock: ExclusiveLock,
    initial: InstanceState,
    child: Arc<Mutex<process::JavaChild>>,
    history: Arc<Mutex<OutputBuffer>>,
    clients: Arc<Mutex<ClientSet>>,
    stopping: Arc<AtomicBool>,
    listener: Listener,
    timeout: Duration,
}

enum Listener {
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixListener),
    #[cfg(windows)]
    Windows(mcpm_exec::ipc::NamedPipeListener),
}

impl Runtime {
    fn prepare(spec: &StartSpec) -> Result<(Self, InstanceState)> {
        let paths = InstancePaths::new(&spec.server_root);
        let lock =
            ExclusiveLock::acquire(paths.lock()).context("server instance is already running")?;
        let socket = SocketAddress::for_identity(paths.socket_identity());
        #[cfg(unix)]
        let listener = Listener::Unix(socket.bind().context("failed to bind instance socket")?);
        #[cfg(windows)]
        let listener = Listener::Windows(
            socket
                .bind_named_pipe()
                .context("failed to bind instance pipe")?,
        );
        let java = process::spawn(spec).context("failed to start Java for server instance")?;
        let initial = InstanceState {
            instance_id: spec.instance_id.clone(),
            shim_pid: std::process::id(),
            java_pid: java.child.id(),
            java_started_at: state::now(),
            socket: paths.socket().display().to_string(),
            plan: spec.plan.clone(),
            status: InstanceStatus::Running,
            exit: None,
            shim_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        };
        state::write(&paths, &initial).context("failed to publish running instance state")?;
        Ok((
            Self {
                paths,
                socket,
                _lock: lock,
                initial: initial.clone(),
                child: Arc::new(Mutex::new(java)),
                history: Arc::new(Mutex::new(OutputBuffer::new(4096))),
                clients: Arc::new(Mutex::new(ClientSet::default())),
                stopping: Arc::new(AtomicBool::new(false)),
                listener,
                timeout: Duration::from_secs(spec.stop_timeout_secs.max(1)),
            },
            initial,
        ))
    }

    fn supervise(self) -> Result<()> {
        self.spawn_output_pumps();
        self.spawn_accept_loop();
        loop {
            let status = {
                let mut child = self.child.lock().expect("instance child mutex poisoned");
                child.child.try_wait().context("failed to wait for Java")?
            };
            if let Some(status) = status {
                let (code, signal) = exit_parts(status);
                let stopped = self.stopping.load(Ordering::Acquire);
                let mut finished = self.initial.clone();
                finished.status = if stopped {
                    InstanceStatus::Stopped
                } else {
                    InstanceStatus::Exited
                };
                finished.exit = Some(ExitRecord {
                    code,
                    signal,
                    stopped,
                    at: state::now(),
                });
                state::write(&self.paths, &finished)?;
                self.broadcast(&Frame::Exit {
                    code,
                    signal,
                    stopped,
                });
                self.broadcast(&Frame::Bye);
                self.socket.cleanup().ok();
                return Ok(());
            }
            thread::sleep(Duration::from_millis(30));
        }
    }

    fn spawn_output_pumps(&self) {
        for (stream_name, output) in [
            ("stdout", OutputKind::Stdout),
            ("stderr", OutputKind::Stderr),
        ] {
            let child = Arc::clone(&self.child);
            let history = Arc::clone(&self.history);
            let clients = Arc::clone(&self.clients);
            thread::spawn(move || {
                let reader = {
                    let mut child = child.lock().expect("instance child mutex poisoned");
                    match output {
                        OutputKind::Stdout => child
                            .child
                            .stdout
                            .take()
                            .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
                        OutputKind::Stderr => child
                            .child
                            .stderr
                            .take()
                            .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
                    }
                };
                if let Some(reader) = reader {
                    pump(reader, stream_name, history, clients);
                }
            });
        }
    }

    fn spawn_accept_loop(&self) {
        let listener = match &self.listener {
            #[cfg(unix)]
            Listener::Unix(listener) => listener
                .try_clone()
                .expect("failed to clone instance listener"),
            #[cfg(windows)]
            Listener::Windows(_) => return,
        };
        let clients = Arc::clone(&self.clients);
        let history = Arc::clone(&self.history);
        let child = Arc::clone(&self.child);
        let stopping = Arc::clone(&self.stopping);
        let timeout = self.timeout;
        thread::spawn(move || {
            loop {
                let stream = listener
                    .accept()
                    .ok()
                    .map(|(stream, _)| LocalStream::Unix(stream));
                let Some(stream) = stream else { break };
                let id = clients
                    .lock()
                    .expect("instance clients mutex poisoned")
                    .insert(
                        stream
                            .try_clone()
                            .expect("failed to clone instance client stream"),
                    );
                let clients_for_client = Arc::clone(&clients);
                let history_for_client = Arc::clone(&history);
                let child_for_client = Arc::clone(&child);
                let stopping_for_client = Arc::clone(&stopping);
                thread::spawn(move || {
                    client(
                        id,
                        stream,
                        clients_for_client,
                        history_for_client,
                        child_for_client,
                        stopping_for_client,
                        timeout,
                    )
                });
            }
        });
    }

    fn broadcast(&self, frame: &Frame) {
        let mut clients = self
            .clients
            .lock()
            .expect("instance clients mutex poisoned");
        let mut streams = std::mem::take(&mut clients.streams);
        drop(clients);
        streams.retain_mut(|(_, stream)| protocol::write(stream, frame).is_ok());
        let mut clients = self
            .clients
            .lock()
            .expect("instance clients mutex poisoned");
        clients.streams.extend(streams);
    }
}

#[derive(Clone, Copy)]
enum OutputKind {
    Stdout,
    Stderr,
}

#[derive(Default)]
struct ClientSet {
    next_id: usize,
    streams: Vec<(usize, LocalStream)>,
}

impl ClientSet {
    fn insert(&mut self, stream: LocalStream) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.streams.push((id, stream));
        id
    }

    fn remove(&mut self, id: usize) {
        self.streams.retain(|(existing, _)| *existing != id);
    }
}

fn client(
    id: usize,
    mut stream: LocalStream,
    clients: Arc<Mutex<ClientSet>>,
    history: Arc<Mutex<OutputBuffer>>,
    child: Arc<Mutex<process::JavaChild>>,
    stopping: Arc<AtomicBool>,
    timeout: Duration,
) {
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => {
            clients
                .lock()
                .expect("instance clients mutex poisoned")
                .remove(id);
            return;
        }
    };
    let mut reader = BufReader::new(reader_stream);
    while let Ok(Some(frame)) = protocol::read(&mut reader) {
        match frame {
            Frame::Hello { tail } => {
                let history = history.lock().expect("instance history mutex poisoned");
                let missing = tail.saturating_sub(history.len());
                if protocol::write(
                    &mut stream,
                    &Frame::Missing {
                        count: missing as u64,
                    },
                )
                .is_err()
                {
                    break;
                }
                for (sequence, stream_name, text) in history.tail(tail) {
                    if protocol::write(
                        &mut stream,
                        &Frame::Line {
                            sequence: *sequence,
                            stream: stream_name.clone(),
                            text: text.clone(),
                        },
                    )
                    .is_err()
                    {
                        break;
                    }
                }
            }
            Frame::Input { line } => {
                if let Ok(mut child) = child.lock() {
                    if let Some(stdin) = child.child.stdin.as_mut() {
                        let _ = input::send(stdin, &line);
                    }
                }
            }
            Frame::Stop { force } => {
                stopping.store(true, Ordering::Release);
                if let Ok(mut child) = child.lock() {
                    let _ = process::terminate(&mut child.child, force);
                    if !force {
                        let started = Instant::now();
                        while started.elapsed() < timeout {
                            if child.child.try_wait().ok().flatten().is_some() {
                                break;
                            }
                            thread::sleep(Duration::from_millis(20));
                        }
                        if child.child.try_wait().ok().flatten().is_none() {
                            let _ = process::terminate(&mut child.child, true);
                        }
                    }
                }
            }
            Frame::Bye => break,
            _ => {}
        }
    }
    clients
        .lock()
        .expect("instance clients mutex poisoned")
        .remove(id);
}

fn pump<R: Read + Send + 'static>(
    reader: R,
    stream_name: &str,
    history: Arc<Mutex<OutputBuffer>>,
    clients: Arc<Mutex<ClientSet>>,
) {
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        let (sequence, _) = history
            .lock()
            .expect("instance history mutex poisoned")
            .push(stream_name, line.clone());
        let frame = Frame::Line {
            sequence,
            stream: stream_name.to_owned(),
            text: line,
        };
        let mut clients_guard = clients.lock().expect("instance clients mutex poisoned");
        let mut streams = std::mem::take(&mut clients_guard.streams);
        drop(clients_guard);
        streams.retain_mut(|(_, stream)| protocol::write(stream, &frame).is_ok());
        let mut clients_guard = clients.lock().expect("instance clients mutex poisoned");
        clients_guard.streams.extend(streams);
    }
}

fn exit_parts(status: std::process::ExitStatus) -> (Option<i32>, Option<i32>) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        (status.code(), status.signal())
    }
    #[cfg(not(unix))]
    {
        (status.code(), None)
    }
}
