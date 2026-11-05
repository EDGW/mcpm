//! Reusable SSH session ownership, secure local dispatch, readiness, and link administration.

use crate::{LinkArgs, LinkCommand};
use anyhow::{Context, Result, ensure};
use mcpm_exec::{
    endpoint::{AlreadyRunning, Endpoint},
    messaging::{Channel, Session, relay},
    operation::{self, Connection, RemoteError},
    service::control::{DaemonHello, Ready, Status, Stop, StopInput, VERSION},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{IsTerminal, Write},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc, task::JoinSet};

mod control;
mod idle;
mod platform;
mod ssh;
use platform::{duplicate_stdout, redirect, shutdown_signal};

pub async fn run(args: LinkArgs) -> Result<()> {
    match &args.command {
        Some(LinkCommand::List) => return list(args.raw).await,
        Some(LinkCommand::Stop {
            ssh,
            all,
            force,
            mcpmd,
        }) => {
            if *all {
                for (_, endpoint) in entries()? {
                    stop(&endpoint, *force).await?;
                }
                return Ok(());
            }
            let connection = Connection {
                ssh: ssh.clone().context("link stop requires SSH or --all")?,
                mcpmd: mcpmd.clone(),
            };
            if connection.mcpmd.is_none() {
                let fingerprint = blake3::hash(connection.ssh.as_bytes()).to_hex().to_string();
                let mut matched = false;
                for (path, endpoint) in entries()? {
                    let record: StateRecord = serde_json::from_slice(&fs::read(path)?)?;
                    if record.ssh_key == fingerprint {
                        stop(&endpoint, *force).await?;
                        matched = true;
                    }
                }
                if matched {
                    return Ok(());
                }
            }
            return stop(&Endpoint::for_key(&connection.key()?)?, *force).await;
        }
        None => {}
    }
    let mut announced = false;
    let mut readiness = if args.ready {
        Some(duplicate_stdout()?)
    } else {
        None
    };
    let result = start(&args, &mut announced, &mut readiness).await;
    if let Err(error) = &result
        && !announced
        && let Some(output) = readiness.as_mut()
    {
        report(output, &Ready::Error(RemoteError::from(error)))?;
    }
    result
}

async fn start(
    args: &LinkArgs,
    announced: &mut bool,
    readiness: &mut Option<fs::File>,
) -> Result<()> {
    let connection = match &args.connection {
        Some(json) => {
            serde_json::from_str::<Connection>(json).context("invalid link connection")?
        }
        None => Connection {
            ssh: args.ssh.clone().context("link requires an SSH command")?,
            mcpmd: args.mcpmd.clone(),
        },
    };
    let endpoint = Endpoint::for_key(&connection.key()?)?;
    let mut listener = match endpoint.listen() {
        Ok(listener) => listener,
        Err(error) if error.is::<AlreadyRunning>() => {
            if let Some(mut output) = readiness.take() {
                report(&mut output, &Ready::AlreadyRunning)?;
                *announced = true;
                return Ok(());
            }
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    crate::logs::init_link(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(endpoint.log_path())?,
    )?;
    let mut command = Command::from(ssh::command(&connection)?);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("failed to start SSH transport")?;
    let stdin = child.stdin.take().context("SSH stdin is unavailable")?;
    let stdout = child.stdout.take().context("SSH stdout is unavailable")?;
    let mut stderr = child.stderr.take().context("SSH stderr is unavailable")?;
    let startup = Arc::new(Mutex::new(Vec::new()));
    let capture_startup = Arc::new(AtomicBool::new(true));
    let stderr_task = tokio::spawn({
        let startup = startup.clone();
        let capture = capture_startup.clone();
        let path = endpoint.log_path();
        async move {
            let mut log = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            let mut buffer = [0u8; 8192];
            loop {
                let read = stderr.read(&mut buffer).await?;
                if read == 0 {
                    return Ok::<_, std::io::Error>(());
                }
                if capture.load(Ordering::SeqCst) {
                    startup.lock().unwrap().extend_from_slice(&buffer[..read]);
                }
                log.write_all(&buffer[..read])?;
            }
        }
    });
    let remote = Arc::new(Session::new((stdout, stdin)));
    let hello = operation::call::<DaemonHello>(&remote, &()).await;
    let hello = match hello {
        Ok(hello) => hello,
        Err(error) => {
            drop(remote);
            let status = child
                .wait()
                .await
                .context("failed to wait for failed SSH startup")?;
            let _ = stderr_task.await;
            let stderr = String::from_utf8_lossy(&startup.lock().unwrap())
                .trim()
                .to_owned();
            return Err(error)
                .with_context(|| format!("SSH startup exited with {status}: {stderr}"));
        }
    };
    ensure!(
        hello.version == VERSION,
        "incompatible remote mcpmd version {}; expected {VERSION}",
        hello.version
    );
    capture_startup.store(false, Ordering::SeqCst);
    startup.lock().unwrap().clear();
    let state = control::State::new();
    let registry = Arc::new(control::registry(state.clone())?);
    let record = StateRecord {
        pid: std::process::id(),
        started: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        connection: connection.summary(),
        ssh_key: blake3::hash(connection.ssh.as_bytes()).to_hex().to_string(),
    };
    let state_path = endpoint.state_path();
    let mut temporary = tempfile::NamedTempFile::new_in(
        state_path
            .parent()
            .context("link state directory is unavailable")?,
    )?;
    serde_json::to_writer(&mut temporary, &record)?;
    temporary
        .persist(&state_path)
        .map_err(|error| error.error)?;
    let _published = PublishedState(state_path);
    redirect(&endpoint.log_path())?;
    if let Some(mut output) = readiness.take() {
        report(&mut output, &Ready::Ready)?;
    }
    *announced = true;
    let (send, mut jobs) = mpsc::unbounded_channel::<Forward>();
    let mut clients = JoinSet::new();
    let mut calls = JoinSet::new();
    let signal = shutdown_signal();
    tokio::pin!(signal);
    let outcome = loop {
        if state.stopping.load(Ordering::SeqCst)
            && (state.force.load(Ordering::SeqCst) || state.calls.load(Ordering::SeqCst) == 0)
        {
            break Ok(());
        }
        tokio::select! {
            accepted = listener.accept(), if !state.stopping.load(Ordering::SeqCst) => {
                let carrier = match accepted { Ok(carrier) => carrier, Err(error) => break Err(error) };
                let lease = state.lease(false);
                let state = state.clone();
                let registry = registry.clone();
                let remote = remote.clone();
                let send = send.clone();
                clients.spawn(async move {
                    let _lease = lease;
                    let mut session = Session::new(carrier);
                    operation::serve_or_forward(&mut session, registry, move |incoming| {
                        if state.stopping.load(Ordering::SeqCst) { return; }
                        let lease = state.lease(true);
                        let _ = send.send(Forward { incoming, outgoing: remote.open(), lease });
                    }).await
                });
            },
            Some(job) = jobs.recv() => {
                calls.spawn(async move {
                    let _lease = job.lease;
                    let _ = relay(job.incoming, job.outgoing).await;
                });
            },
            _ = clients.join_next(), if !clients.is_empty() => {},
            _ = calls.join_next(), if !calls.is_empty() => {},
            _ = state.changed.notified() => {},
            _ = idle::expired(&state, args.idle_timeout), if !state.stopping.load(Ordering::SeqCst) => state.stop(false),
            _ = &mut signal, if !state.stopping.load(Ordering::SeqCst) => state.stop(false),
            ended = remote.closed() => {
                state.ssh_alive.store(false, Ordering::SeqCst);
                break match ended {
                    Ok(()) => Err(anyhow::anyhow!("SSH session closed unexpectedly")),
                    Err(error) => Err(anyhow::Error::new(error).context("SSH session failed")),
                };
            },
        }
    };
    clients.abort_all();
    calls.abort_all();
    while clients.join_next().await.is_some() {}
    while calls.join_next().await.is_some() {}
    drop(jobs);
    drop(send);
    drop(remote);
    drop(listener);
    if outcome.is_err() || state.force.load(Ordering::SeqCst) {
        let _ = child.start_kill();
    }
    let status = child
        .wait()
        .await
        .context("failed to wait for SSH transport shutdown")?;
    let _ = stderr_task.await;
    outcome?;
    ensure!(
        status.success() || state.force.load(Ordering::SeqCst),
        "SSH transport exited with {status}"
    );
    Ok(())
}

struct Forward {
    incoming: Channel,
    outgoing: Channel,
    lease: control::Lease,
}
#[derive(Serialize, Deserialize)]
struct StateRecord {
    pid: u32,
    started: u64,
    connection: String,
    ssh_key: String,
}
struct PublishedState(PathBuf);
impl Drop for PublishedState {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn report(output: &mut fs::File, ready: &Ready) -> Result<()> {
    serde_json::to_writer(&mut *output, ready)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn entries() -> Result<Vec<(PathBuf, Endpoint)>> {
    let mut entries = Vec::new();
    let mut keys = HashSet::new();
    for directory in Endpoint::directories() {
        let files = match fs::read_dir(&directory) {
            Ok(files) => files,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("failed to enumerate link state files"),
        };
        for file in files {
            let path = file?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(key) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if key.len() != 16
                || !key.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !keys.insert(key.to_owned())
            {
                continue;
            }
            let endpoint = Endpoint::for_key(key)?;
            entries.push((path, endpoint));
        }
    }
    Ok(entries)
}

async fn list(raw: bool) -> Result<()> {
    for (path, endpoint) in entries()? {
        let result = async {
            let record: StateRecord = serde_json::from_slice(&fs::read(&path)?)?;
            let session = Session::new(endpoint.connect().await?);
            let status = operation::call::<Status>(&session, &()).await?;
            Ok::<_, anyhow::Error>((record, status))
        };
        match tokio::time::timeout(Duration::from_secs(3), result).await {
            Ok(Ok((record, status))) => {
                if !raw && std::io::stdout().is_terminal() {
                    println!(
                        "{}: pid {}, {} clients, {} calls, idle {}s, SSH {} ({})",
                        endpoint.key(),
                        record.pid,
                        status.connections.saturating_sub(1),
                        status.calls,
                        status.idle_seconds,
                        if status.ssh_alive {
                            "connected"
                        } else {
                            "closed"
                        },
                        record.connection
                    );
                } else {
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        endpoint.key(),
                        record.pid,
                        status.connections.saturating_sub(1),
                        status.calls,
                        status.idle_seconds,
                        status.ssh_alive,
                        record.connection
                    );
                }
            }
            _ => {
                println!("{}\tstale", endpoint.key());
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

async fn stop(endpoint: &Endpoint, force: bool) -> Result<()> {
    let session = Session::new(endpoint.connect().await?);
    operation::call::<Stop>(&session, &StopInput { force }).await?;
    Ok(())
}
