//! Multiplexed connection holder for one SSH `mcpmd serve` session.
//!
//! The holder parses only protocol envelopes and GUIDs. It never decodes operation payloads;
//! each request is forwarded unchanged to the remote serve process and replies return to the
//! client that owns the GUID.

#[cfg(any(unix, windows))]
use std::{collections::{HashMap, HashSet}, io::{BufRead, BufReader, Write}, process::{Child, Stdio}, sync::mpsc::{self, Receiver, Sender, TryRecvError}, thread, time::{Duration, Instant}};
use thiserror::Error;

#[cfg(unix)]

/// Client-side holder connection settings.
#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub ssh: String,
    pub executable: String,
    pub holder_executable: std::path::PathBuf,
    pub build: String,
    pub timeout: Duration,
}

/// Connects to the identity's holder, starting `mcpmd link` when absent.
#[cfg(unix)]
pub fn connect(options: ClientOptions) -> Result<std::os::unix::net::UnixStream, LinkError> {
    let identity = format!("{}\0{}\0{}", options.ssh, options.executable, options.build);
    let address = SocketAddress::for_identity(identity.as_bytes());
    if let Ok(stream) = address.connect() { return Ok(stream); }
    let _child = crate::ipc::Detached::new(&options.holder_executable)
        .args(["link", "--ssh", &options.ssh, "--executable", &options.executable, "--build", &options.build])
        .spawn()
        .map_err(|error| LinkError::Startup(error.to_string()))?;
    let deadline = Instant::now() + options.timeout;
    loop {
        match address.connect() {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() < deadline => { thread::sleep(Duration::from_millis(25)); let _ = error; }
            Err(error) => return Err(LinkError::Socket(error)),
        }
    }
}

#[cfg(windows)]
pub fn connect(options: ClientOptions) -> Result<crate::ipc::LocalStream, LinkError> {
    let identity = format!("{}\0{}\0{}", options.ssh, options.executable, options.build);
    let address = SocketAddress::for_identity(identity.as_bytes());
    if let Ok(stream) = address.connect_named_pipe() { return Ok(crate::ipc::LocalStream::NamedPipe(stream)); }
    let _child = crate::ipc::Detached::new(&options.holder_executable)
        .args(["link", "--ssh", &options.ssh, "--executable", &options.executable, "--build", &options.build])
        .spawn().map_err(|error| LinkError::Startup(error.to_string()))?;
    let deadline = Instant::now() + options.timeout;
    loop {
        match address.connect_named_pipe() {
            Ok(stream) => return Ok(crate::ipc::LocalStream::NamedPipe(stream)),
            Err(error) if Instant::now() < deadline => { thread::sleep(Duration::from_millis(25)); let _ = error; }
            Err(error) => return Err(LinkError::Socket(error)),
        }
    }
}

use crate::{ipc::{ExclusiveLock, SocketAddress}, protocol::{self, Frame, Message}};
use crate::ssh::Connection;

#[derive(Clone, Debug)]
pub struct Options {
    pub ssh: String,
    pub executable: String,
    pub build: String,
    pub idle_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self { Self { ssh: String::new(), executable: "mcpmd".into(), build: String::new(), idle_timeout: Duration::from_secs(300) } }
}

pub fn run(options: Options) -> Result<(), LinkError> {
    #[cfg(unix)] { run_unix(options) }
    #[cfg(windows)] { run_windows(options) }
}

#[cfg(unix)]
fn run_unix(options: Options) -> Result<(), LinkError> {
    let identity = format!("{}\0{}\0{}", options.ssh, options.executable, options.build);
    let address = SocketAddress::for_identity(identity.as_bytes());
    let lock = ExclusiveLock::acquire(address.path().with_extension("lock")).map_err(LinkError::Lock)?;
    address.cleanup().map_err(LinkError::Socket)?;
    let listener = address.bind().map_err(LinkError::Socket)?;
    listener.set_nonblocking(true).map_err(LinkError::Io)?;
    let (remote_tx, remote_rx) = mpsc::channel::<RemoteEvent>();
    let mut child = start_ssh(&options, remote_tx.clone())?;
    let mut remote_input = child.stdin.take().ok_or(LinkError::MissingPipe("stdin"))?;
    let initial = remote_rx.recv_timeout(Duration::from_secs(30)).map_err(|_| LinkError::Ready { stderr: String::new() })?;
    let ready = match initial {
        RemoteEvent::Frame(frame) if frame.status() == protocol::Status::Ready => {
            let payload: protocol::Ready = frame.decode().map_err(|error| LinkError::Protocol(format!("invalid Ready frame: {error}")))?;
            if payload.version != protocol::VERSION { return Err(LinkError::Protocol(format!("unsupported remote protocol version {}; expected {}", payload.version, protocol::VERSION))); }
            frame
        }
        RemoteEvent::Exit(stderr) => return Err(LinkError::Ready { stderr }),
        _ => return Err(LinkError::Ready { stderr: String::new() }),
    };
    let ready_line = encode(&ready)?;
    let mut clients: HashMap<u64, Client> = HashMap::new();
    let mut owners: HashMap<uuid::Uuid, u64> = HashMap::new();
    let mut next_client = 1_u64;
    let mut draining = false;
    let mut idle_since: Option<Instant> = None;
    loop {
        while let Ok(event) = remote_rx.try_recv() {
            match event {
                RemoteEvent::Frame(frame) => {
                    let guid = frame.guid();
                    if guid.is_nil() { continue; }
                    if let Some(owner) = owners.get(&guid).copied() {
                        if let Some(client) = clients.get(&owner) { let _ = client.output.send(Ok(encode(&frame)?)); }
                        if matches!(frame.status(), protocol::Status::Finished | protocol::Status::Failed) { owners.remove(&guid); }
                    }
                }
                RemoteEvent::Exit(stderr) => {
                    for client in clients.values() { let _ = client.output.send(Err(LinkError::SshExit { status: "disconnected".into(), stderr: stderr.clone() })); }
                    return finish(address, lock, child);
                }
            }
        }
        while let Ok((stream, _)) = listener.accept() {
            if draining { continue; }
            let id = next_client; next_client += 1;
            let (commands, events) = spawn_client(crate::ipc::LocalStream::Unix(stream), ready_line.clone());
            clients.insert(id, Client { output: commands, events, guids: HashSet::new() });
        }
        let ids: Vec<u64> = clients.keys().copied().collect();
        for id in ids {
            let mut disconnected = false;
            while let Some(client) = clients.get_mut(&id) {
                match client.events.try_recv() {
                    Ok(ClientEvent::Message(message)) => {
                        let guid = match &message { Message::Request(request) => request.guid, Message::Cancel(cancel) => cancel.cancel };
                        if let Message::Request(_) = &message { owners.insert(guid, id); client.guids.insert(guid); }
                        let bytes = serde_json::to_vec(&message).map_err(|error| LinkError::Protocol(error.to_string()))?;
                        remote_input.write_all(&bytes).map_err(LinkError::Io)?; remote_input.write_all(b"\n").map_err(LinkError::Io)?; remote_input.flush().map_err(LinkError::Io)?;
                    }
                    Ok(ClientEvent::Management(command)) => {
                        let response = match command.as_str() {
                            "list" => format!("clients={} pending={} draining={}\n", clients.len(), owners.len(), draining).into_bytes(),
                            "stop" => { draining = true; b"stopping\n".to_vec() }
                            _ => b"unknown management command\n".to_vec(),
                        };
                        if let Some(client) = clients.get(&id) { let _ = client.output.send(Ok(response)); }
                    }
                    Ok(ClientEvent::Closed) => { disconnected = true; break; }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => { disconnected = true; break; }
                }
            }
            if disconnected {
                if let Some(client) = clients.remove(&id) {
                    for guid in client.guids {
                        if owners.remove(&guid).is_some() {
                            let message = Message::Cancel(protocol::Cancel { cancel: guid });
                            let bytes = serde_json::to_vec(&message).map_err(|error| LinkError::Protocol(error.to_string()))?;
                            remote_input.write_all(&bytes).map_err(LinkError::Io)?; remote_input.write_all(b"\n").map_err(LinkError::Io)?;
                        }
                    }
                }
            }
        }
        if clients.is_empty() && owners.is_empty() {
            idle_since.get_or_insert_with(Instant::now);
            if idle_since.is_some_and(|since| since.elapsed() >= options.idle_timeout) { break; }
        } else { idle_since = None; }
        if draining && owners.is_empty() { break; }
        thread::sleep(Duration::from_millis(10));
    }
    drop(remote_input);
    let _ = child.wait();
    finish(address, lock, child)
}
#[cfg(windows)]
fn run_windows(options: Options) -> Result<(), LinkError> {
    let identity = format!("{}\0{}\0{}", options.ssh, options.executable, options.build);
    let address = SocketAddress::for_identity(identity.as_bytes());
    let lock = ExclusiveLock::acquire(address.path().with_extension("lock")).map_err(LinkError::Lock)?;
    let listener = address.bind_named_pipe().map_err(LinkError::Socket)?;
    let (remote_tx, remote_rx) = mpsc::channel::<RemoteEvent>();
    let mut child = start_ssh(&options, remote_tx)?;
    let mut remote_input = child.stdin.take().ok_or(LinkError::MissingPipe("stdin"))?;
    let ready = match remote_rx.recv_timeout(Duration::from_secs(30)).map_err(|_| LinkError::Ready { stderr: String::new() })? {
        RemoteEvent::Frame(frame) if frame.status() == protocol::Status::Ready => frame,
        RemoteEvent::Exit(stderr) => return Err(LinkError::Ready { stderr }),
        _ => return Err(LinkError::Ready { stderr: String::new() }),
    };
    let ready_line = encode(&ready)?;
    let mut stream = listener.accept_stream().map_err(LinkError::Socket)?;
    let (output, events) = spawn_client(crate::ipc::LocalStream::NamedPipe(stream), ready_line);
    let mut client = Client { output, events, guids: HashSet::new() };
    loop {
        match client.events.recv_timeout(Duration::from_millis(50)) {
            Ok(ClientEvent::Message(message)) => {
                let bytes = serde_json::to_vec(&message).map_err(|error| LinkError::Protocol(error.to_string()))?;
                remote_input.write_all(&bytes).map_err(LinkError::Io)?; remote_input.write_all(b"\n").map_err(LinkError::Io)?;
            }
            Ok(ClientEvent::Management(command)) if command == "stop" => break,
            Ok(ClientEvent::Closed) => break,
            Err(TryRecvError::Timeout) => {
                if let Ok(RemoteEvent::Exit(stderr)) = remote_rx.try_recv() { return Err(LinkError::SshExit { status: "disconnected".into(), stderr }); }
            }
            Err(TryRecvError::Disconnected) => break,
            Ok(_) => {}
        }
    }
    drop(remote_input); let _ = child.wait(); finish(address, lock, child)
}

#[cfg(any(unix, windows))]
struct Client { output: Sender<Result<Vec<u8>, LinkError>>, events: Receiver<ClientEvent>, guids: HashSet<uuid::Uuid> }

#[cfg(any(unix, windows))]
enum ClientEvent { Message(Message), Management(String), Closed }

#[cfg(any(unix, windows))]
enum RemoteEvent { Frame(Frame), Exit(String) }

#[cfg(any(unix, windows))]
fn spawn_client(stream: crate::ipc::LocalStream, ready: Vec<u8>) -> (Sender<Result<Vec<u8>, LinkError>>, Receiver<ClientEvent>) {
    let (output, output_rx) = mpsc::channel::<Result<Vec<u8>, LinkError>>();
    let (events, event_rx) = mpsc::channel();
    let mut writer = stream.try_clone().expect("local stream clone");
    thread::spawn(move || { let _ = writer.write_all(&ready); let _ = writer.flush(); while let Ok(message) = output_rx.recv() { match message { Ok(bytes) => { if writer.write_all(&bytes).is_err() { break; } let _ = writer.flush(); }, Err(_) => break } } });
    thread::spawn(move || { let mut reader = BufReader::new(stream); let mut line = Vec::new(); loop { line.clear(); match reader.read_until(b'\n', &mut line) { Ok(0) => { let _ = events.send(ClientEvent::Closed); break; }, Ok(_) => match serde_json::from_slice::<Message>(&line) { Ok(message) => { if events.send(ClientEvent::Message(message)).is_err() { break; } }, Err(_) => { if let Ok(command) = serde_json::from_slice::<Management>(&line) { if events.send(ClientEvent::Management(command.command)).is_err() { break; } } } }, Err(_) => { let _ = events.send(ClientEvent::Closed); break; } } } });
    (output, event_rx)
}

#[cfg(any(unix, windows))]
#[derive(serde::Deserialize)]
struct Management { command: String }

#[cfg(any(unix, windows))]
fn start_ssh(options: &Options, events: Sender<RemoteEvent>) -> Result<Child, LinkError> {
    let connection = Connection::new(&options.ssh, &options.executable);
    let mut command = connection.command().map_err(LinkError::Connection)?;
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|source| LinkError::Ssh { source })?;
    let stdout = child.stdout.take().ok_or(LinkError::MissingPipe("stdout"))?;
    let stderr = child.stderr.take().ok_or(LinkError::MissingPipe("stderr"))?;
    let frame_events = events.clone();
    thread::spawn(move || { let mut reader = BufReader::new(stdout); loop { match protocol::read::<Frame>(&mut reader) { Ok(Some(frame)) => { if frame_events.send(RemoteEvent::Frame(frame)).is_err() { break; } }, Ok(None) => { let _ = frame_events.send(RemoteEvent::Exit(String::new())); break; }, Err(error) => { let _ = frame_events.send(RemoteEvent::Exit(error.to_string())); break; } } } });
    thread::spawn(move || { let mut reader = BufReader::new(stderr); let mut text = String::new(); let _ = reader.read_line(&mut text); });
    Ok(child)
}

#[cfg(any(unix, windows))]
fn encode(frame: &Frame) -> Result<Vec<u8>, LinkError> { let mut bytes = serde_json::to_vec(frame).map_err(|error| LinkError::Protocol(error.to_string()))?; bytes.push(b'\n'); Ok(bytes) }

#[cfg(any(unix, windows))]
fn finish(address: SocketAddress, lock: ExclusiveLock, mut child: Child) -> Result<(), LinkError> { let _ = child.kill(); let _ = child.wait(); address.cleanup().map_err(LinkError::Socket)?; drop(lock); Ok(()) }

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("link holder is not supported on this platform")] Unsupported,
    #[error("failed to acquire link lock: {0}")] Lock(#[source] crate::ipc::LockError),
    #[error("link socket failed: {0}")] Socket(#[source] crate::ipc::SocketError),
    #[error("invalid SSH connection: {0}")] Connection(#[source] crate::ssh::ConnectionError),
    #[error("failed to start link holder: {0}")]
    Startup(String),
    #[error("failed to start SSH: {source}; configure a key or agent for non-interactive authentication")] Ssh { source: std::io::Error },
    #[error("SSH serve did not send Ready: {stderr}")] Ready { stderr: String },
    #[error("SSH serve exited: {status}: {stderr}")] SshExit { status: String, stderr: String },
    #[error("SSH process did not provide {0} pipe")] MissingPipe(&'static str),
    #[error("link I/O failed: {0}")] Io(#[source] std::io::Error),
    #[error("protocol failure: {0}")] Protocol(String),
}
