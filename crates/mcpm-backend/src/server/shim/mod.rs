//! Private shim bootstrap, same-user IPC, instance discovery and terminal receipts.

use crate::operations::server::shim::*;
use anyhow::{Context, Result, bail, ensure};
use mcpm_exec::{endpoint::Endpoint, error::ErrorReport};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod events;
mod history;
mod logs;
mod runtime;

#[derive(Serialize, Deserialize)]
pub(crate) enum Request {
    Inspect(InspectInput),
    Command(CommandInput),
    Logs(LogsInput),
    Status(StatusInput),
    Commands(CommandsInput),
    Kill(ControlInput),
    LaunchResult { run_id: String, request_id: String },
}
impl Request {
    fn location(&self) -> Option<(&Path, Option<&str>)> {
        let reference = match self {
            Self::Inspect(input) => {
                return input
                    .server
                    .as_deref()
                    .map(|server| (server, input.run_id.as_deref()));
            }
            Self::Command(input) => &input.reference,
            Self::Logs(input) => &input.reference,
            Self::Status(input) => &input.reference,
            Self::Commands(input) => &input.reference,
            Self::Kill(input) => &input.reference,
            Self::LaunchResult { .. } => return None,
        };
        Some((&reference.server, Some(&reference.run_id)))
    }
    fn set_server(&mut self, server: PathBuf) {
        match self {
            Self::Inspect(input) => input.server = Some(server),
            Self::Command(input) => input.reference.server = server,
            Self::Logs(input) => input.reference.server = server,
            Self::Status(input) => input.reference.server = server,
            Self::Commands(input) => input.reference.server = server,
            Self::Kill(input) => input.reference.server = server,
            Self::LaunchResult { .. } => {}
        }
    }
    fn run_id(&self) -> Option<&str> {
        match self {
            Self::Inspect(input) => input.run_id.as_deref(),
            Self::Command(input) => Some(&input.reference.run_id),
            Self::Logs(input) => Some(&input.reference.run_id),
            Self::Status(input) => Some(&input.reference.run_id),
            Self::Commands(input) => Some(&input.reference.run_id),
            Self::Kill(input) => Some(&input.reference.run_id),
            Self::LaunchResult { run_id, .. } => Some(run_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RunReference {
    run_id: String,
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    reference: RunReference,
    key: String,
    launch_request_id: String,
}
#[derive(Serialize, Deserialize)]
struct Bootstrap {
    input: LaunchInput,
    record: Record,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    snapshot: RunSnapshot,
}

/// Durable exit data deliberately has no server path; the containing folder is its location.
#[derive(Serialize, Deserialize)]
struct RunSnapshot {
    reference: RunReference,
    state: State,
    exit_code: Option<u32>,
    communication: Communication,
    actual_channel: Channel,
    stdout: CursorRange,
    stderr: CursorRange,
}
impl From<Snapshot> for RunSnapshot {
    fn from(value: Snapshot) -> Self {
        Self {
            reference: RunReference {
                run_id: value.reference.run_id,
            },
            state: value.state,
            exit_code: value.exit_code,
            communication: value.communication,
            actual_channel: value.actual_channel,
            stdout: value.stdout,
            stderr: value.stderr,
        }
    }
}
impl From<RunSnapshot> for Snapshot {
    fn from(value: RunSnapshot) -> Self {
        Self {
            reference: ShimRef {
                server: PathBuf::new(),
                run_id: value.reference.run_id,
            },
            state: value.state,
            exit_code: value.exit_code,
            communication: value.communication,
            actual_channel: value.actual_channel,
            server_status: ServerStatus::Unknown,
            stdout: value.stdout,
            stderr: value.stderr,
        }
    }
}

/// Invoked only by the thin mcpmd shim entry. Bootstrap never appears on argv or disk.
pub async fn serve() -> Result<()> {
    let mut input = tokio::io::stdin();
    let bootstrap: Bootstrap = read_frame(&mut input)
        .await
        .context("failed to receive shim bootstrap")?;
    drop(input);
    let result = runtime::start(bootstrap).await;
    if let Err(error) = &result {
        let reply: std::result::Result<Launched, ErrorReport> = Err(ErrorReport::capture(error));
        let _ = write_frame(&mut tokio::io::stdout(), &reply).await;
    }
    result
}

pub(crate) async fn launch(mut input: LaunchInput) -> Result<Launched> {
    input.server = std::fs::canonicalize(&input.server).context("failed to resolve server root")?;
    crate::workspace::require_local(&input.server)?;
    ensure!(
        !input.launch_request_id.is_empty() && input.launch_request_id.len() <= 128,
        "invalid launch request id"
    );
    // Check RCON configuration before creating a shim; this never authenticates or runs Java.
    if input.communication == Communication::Rcon {
        let settings =
            mcpm_lib::local::mc::server::launch::rcon::Settings::from_plan(&input.plan.decode()?)?;
        ensure!(
            input.communication != Communication::Rcon || settings.is_some(),
            "rcon mode requires enabled RCON and credentials in Minecraft properties"
        );
    }
    let directory = run_directory(&input.server)?;
    let _launch_lock =
        lock(&directory.join("launch.lock")).context("another launch is being prepared")?;
    if let Some(record) = read_optional::<Record>(&directory.join("instance.json"))? {
        ensure!(record.version == 1, "unsupported shim discovery version");
        // A matching receipt proves this run ended, even after the folder was moved.
        // Never send a stale record to an endpoint now owned by another server.
        let finished =
            read_optional::<Receipt>(&directory.join("finished.json"))?.filter(|receipt| {
                receipt.snapshot.reference == record.reference
                    && receipt.snapshot.state == State::Exited
            });
        publish(&directory.join("instance.json"), &record)?;
        if let Some(receipt) = finished {
            publish(&directory.join("finished.json"), &receipt)?;
            match std::fs::remove_file(run_hint_path(&record.reference.run_id)?) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("failed to remove completed run hint"),
            }
            ensure!(
                record.launch_request_id != input.launch_request_id,
                "this launch request already exited; a new request id is required"
            );
        } else {
            ensure!(
                instance_locked(&directory)?,
                "shim is unmanaged or this is a copied running record; Java's exit was not confirmed"
            );
            publish(&run_hint_path(&record.reference.run_id)?, &record)?;
            let mut launched: Launched = serde_json::from_value(
                request(
                    &record,
                    &Request::LaunchResult {
                        run_id: record.reference.run_id.clone(),
                        request_id: input.launch_request_id.clone(),
                    },
                )
                .await?,
            )
            .context("invalid existing launch result")?;
            launched.reference.server = input.server;
            return Ok(launched);
        }
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    let mut hash = blake3::Hasher::new();
    hash.update(b"mcpm.server.shim\0");
    hash.update(input.server.as_os_str().as_encoded_bytes());
    let key = hash.finalize().to_hex()[..16].to_string();
    let record = Record {
        version: 1,
        reference: RunReference { run_id },
        key,
        launch_request_id: input.launch_request_id.clone(),
    };
    let bootstrap = Bootstrap { input, record };
    let executable = daemon_executable()?;
    let diagnostic = private_file(&directory.join("shim.log"))?;
    let mut command = tokio::process::Command::new(&executable);
    command
        .arg("shim")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(diagnostic);
    detach(&mut command);
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start shim host {executable:?}"))?;
    let result = async {
        let mut stdin = child.stdin.take().context("missing shim bootstrap pipe")?;
        write_frame(&mut stdin, &bootstrap).await?;
        stdin.shutdown().await?;
        drop(stdin);
        let mut stdout = child.stdout.take().context("missing shim readiness pipe")?;
        let reply: std::result::Result<Launched, ErrorReport> =
            tokio::time::timeout(Duration::from_secs(30), read_frame(&mut stdout))
                .await
                .context("shim did not acknowledge Java spawn; its state is unknown")??;
        reply.map_err(ErrorReport::into_error)
    }
    .await;
    // Tokio's child handle does not kill on drop. Reap the independent shim when possible.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    result.map(|mut launched| {
        launched.reference.server = bootstrap.input.server;
        launched
    })
}

fn run_hint_path(run_id: &str) -> Result<PathBuf> {
    ensure!(!run_id.is_empty() && run_id.len() <= 128, "invalid run id");
    let mut hash = blake3::Hasher::new();
    hash.update(b"mcpm.server.shim.run\0");
    hash.update(run_id.as_bytes());
    let key = hash.finalize().to_hex();
    // Not a link state .json: link discovery must not interpret shim hints.
    Ok(Endpoint::for_key(&key[..16])?
        .state_path()
        .with_extension("shim"))
}

/// Exclusive live-run discovery hint; neither its content nor its name contains a server path.
struct RunHint {
    path: PathBuf,
}
impl RunHint {
    fn create(record: &Record) -> Result<Self> {
        let path = run_hint_path(&record.reference.run_id)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&path)
            .context("RunHintCollision: run lookup address is already occupied; refusing launch")?;
        let hint = Self { path };
        serde_json::to_writer(&mut file, record)?;
        file.flush()?;
        Ok(hint)
    }
}
impl Drop for RunHint {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) async fn call<T: DeserializeOwned>(
    mut request_input: Request,
) -> Result<(T, Option<PathBuf>)> {
    let (record, server) = locate(&mut request_input)?;
    if let Some(server) = &server {
        let directory = server.join(".mcpm/run");
        if let Some(receipt) = read_optional::<Receipt>(&directory.join("finished.json"))?
            && receipt.snapshot.reference == record.reference
            && receipt.snapshot.state == State::Exited
        {
            return Ok((
                serde_json::from_value(receipt.reply(request_input)?)?,
                Some(server.clone()),
            ));
        }
        ensure!(
            instance_locked(&directory)?,
            "shim is unmanaged or this is a copied running record; Java's exit was not confirmed"
        );
    }
    match request(&record, &request_input).await {
        Ok(value) => Ok((
            serde_json::from_value(value).context("invalid shim response")?,
            server,
        )),
        Err(error) => {
            if error.is::<Rejected>() {
                return Err(error);
            }
            if let Some(server) = &server
                && let Some(receipt) =
                    read_optional::<Receipt>(&server.join(".mcpm/run/finished.json"))?
                && receipt.snapshot.reference == record.reference
                && receipt.snapshot.state == State::Exited
            {
                return Ok((
                    serde_json::from_value(receipt.reply(request_input)?)?,
                    Some(server.clone()),
                ));
            }
            Err(error).context("shim unreachable; Java may still be running (unmanaged)")
        }
    }
}

fn locate(request_input: &mut Request) -> Result<(Record, Option<PathBuf>)> {
    let server = request_input
        .location()
        .map(|(server, _)| server)
        .filter(|server| !server.as_os_str().is_empty())
        .map(std::fs::canonicalize)
        .transpose()
        .context("failed to resolve server root")?;
    let record: Record = if let Some(server) = &server {
        read_optional(&server.join(".mcpm/run/instance.json"))?
            .context("no shim instance; launch the server first")?
    } else {
        let run_id = request_input
            .run_id()
            .context("operation requires server or run_id")?;
        read_optional(&run_hint_path(run_id)?)?.context("shim run id was not found")?
    };
    ensure!(record.version == 1, "unsupported shim discovery version");
    ensure!(
        request_input
            .run_id()
            .is_none_or(|id| id == record.reference.run_id),
        "run id no longer identifies this server instance"
    );
    if let Request::Inspect(input) = request_input {
        input.run_id = Some(record.reference.run_id.clone());
    }
    if let Some(server) = &server {
        request_input.set_server(server.clone());
    }
    Ok((record, server))
}

pub(crate) async fn subscribe_logs(
    input: LogsInput,
    sender: mcpm_exec::operation::StreamSender<LogBatch>,
) -> Result<()> {
    let mut request_input = Request::Logs(input);
    let (record, server) = locate(&mut request_input)?;
    if let Some(server) = &server {
        if let Some(receipt) = read_optional::<Receipt>(&server.join(".mcpm/run/finished.json"))?
            && receipt.snapshot.reference == record.reference
            && receipt.snapshot.state == State::Exited
        {
            let Request::Logs(input) = request_input else {
                unreachable!()
            };
            return retained_logs(receipt, &server.join(".mcpm/run"), input, sender).await;
        }
        ensure!(
            instance_locked(&server.join(".mcpm/run"))?,
            "shim is unmanaged or this is a copied running record; Java's exit was not confirmed"
        );
    }
    let endpoint = Endpoint::for_key(&record.key)?;
    let connection = tokio::time::timeout(Duration::from_secs(2), endpoint.connect())
        .await
        .context("shim log channel connection timed out")?;
    let (mut reader, mut writer) = match connection {
        Ok(connection) => connection,
        Err(error) => {
            if let Some(server) = &server
                && let Some(receipt) =
                    read_optional::<Receipt>(&server.join(".mcpm/run/finished.json"))?
                && receipt.snapshot.reference == record.reference
                && receipt.snapshot.state == State::Exited
            {
                let Request::Logs(input) = request_input else {
                    unreachable!()
                };
                return retained_logs(receipt, &server.join(".mcpm/run"), input, sender).await;
            }
            return Err(error).context("shim log channel unreachable; Java may still be running");
        }
    };
    write_frame(&mut writer, &request_input).await?;
    loop {
        let reply: std::result::Result<LogBatch, ErrorReport> = read_frame(&mut reader)
            .await
            .context("log channel closed before the source finished")?;
        let batch = reply.map_err(ErrorReport::into_error)?;
        let finished = batch.finished;
        sender.send(batch).await?;
        if finished {
            return Ok(());
        }
    }
}

pub(crate) async fn subscribe_status(
    input: StatusInput,
    sender: mcpm_exec::operation::StreamSender<Snapshot>,
) -> Result<()> {
    let timeout = input.timeout();
    let mut request_input = Request::Status(input);
    let (record, server) = locate(&mut request_input)?;
    if let Some(snapshot) = terminal_status(&record, server.as_deref())? {
        sender.send(snapshot).await?;
        return Ok(());
    }
    if let Some(server) = &server {
        ensure!(
            instance_locked(&server.join(".mcpm/run"))?,
            "shim is unmanaged or this is a copied running record; Java's exit was not confirmed"
        );
    }
    let endpoint = Endpoint::for_key(&record.key)?;
    let connection = tokio::time::timeout(Duration::from_secs(2), endpoint.connect())
        .await
        .context("state Channel connection timed out")?;
    let (mut reader, mut writer) = match connection {
        Ok(connection) => connection,
        Err(error) => {
            if let Some(snapshot) = terminal_status(&record, server.as_deref())? {
                sender.send(snapshot).await?;
                return Ok(());
            }
            return Err(error).context("state Channel unreachable; Java may still be running");
        }
    };
    write_frame(&mut writer, &request_input).await?;
    loop {
        let reply: std::result::Result<Snapshot, ErrorReport> =
            tokio::time::timeout(timeout, read_frame(&mut reader))
                .await
                .context("state Channel heartbeat timed out; Java's exit is not confirmed")?
                .context("state Channel disconnected before a final Java exit snapshot")?;
        let mut snapshot = reply.map_err(ErrorReport::into_error)?;
        if let Some(server) = &server {
            snapshot.reference.server = server.clone();
        }
        let exited = snapshot.state == State::Exited;
        sender.send(snapshot).await?;
        if exited {
            return Ok(());
        }
    }
}

/// Forwards a live-only subscription; Ready is sent only after the shim registered its cursor.
pub(crate) async fn subscribe_commands(
    input: CommandsInput,
    sender: mcpm_exec::operation::StreamSender<CommandEvent>,
) -> Result<()> {
    let mut request_input = Request::Commands(input);
    let (record, server) = locate(&mut request_input)?;
    if terminal_status(&record, server.as_deref())?.is_some() {
        sender.send(CommandEvent::Ready).await?;
        sender.send(CommandEvent::Done).await?;
        return Ok(());
    }
    if let Some(server) = &server {
        ensure!(
            instance_locked(&server.join(".mcpm/run"))?,
            "command results require the original managed instance"
        );
    }
    let (mut reader, mut writer) = Endpoint::for_key(&record.key)?.connect().await?;
    write_frame(&mut writer, &request_input).await?;
    loop {
        let reply: std::result::Result<CommandEvent, ErrorReport> =
            read_frame(&mut reader)
                .await
                .context("command result Channel disconnected before completion")?;
        let event = reply.map_err(ErrorReport::into_error)?;
        let done = matches!(event, CommandEvent::Done);
        sender.send(event).await?;
        if done {
            return Ok(());
        }
    }
}

fn terminal_status(record: &Record, server: Option<&Path>) -> Result<Option<Snapshot>> {
    let Some(server) = server else {
        return Ok(None);
    };
    let Some(receipt) = read_optional::<Receipt>(&server.join(".mcpm/run/finished.json"))? else {
        return Ok(None);
    };
    if receipt.snapshot.reference != record.reference || receipt.snapshot.state != State::Exited {
        return Ok(None);
    }
    let mut snapshot = Snapshot::from(receipt.snapshot);
    snapshot.reference.server = server.to_owned();
    Ok(Some(snapshot))
}

async fn retained_logs(
    receipt: Receipt,
    directory: &Path,
    mut input: LogsInput,
    sender: mcpm_exec::operation::StreamSender<LogBatch>,
) -> Result<()> {
    let range = match input.source {
        Source::Stdout => &receipt.snapshot.stdout,
        Source::Stderr => &receipt.snapshot.stderr,
    };
    let buffer = logs::Buffer::restore(
        directory,
        &receipt.snapshot.reference.run_id,
        input.source,
        range,
    )?;
    loop {
        let batch = buffer.read(&input)?;
        input.cursor = Some(batch.next_cursor);
        let finished = batch.finished;
        sender.send(batch).await?;
        if finished {
            return Ok(());
        }
    }
}
impl Receipt {
    fn reply(self, request: Request) -> Result<Value> {
        match request {
            Request::Inspect(_) => Ok(serde_json::to_value(Snapshot::from(self.snapshot))?),
            Request::Kill(_) => Ok(serde_json::to_value(KillResult::AlreadyExited)?),
            Request::Command(_) => Ok(serde_json::to_value(CommandResult {
                status: CommandStatus::Exited,
                channel: self.snapshot.actual_channel,
                output: None,
                result: CommandOutcome::Unknown,
                diagnostic: Some("Java has exited".into()),
            })?),
            _ => bail!("Java has exited"),
        }
    }
}
async fn request(record: &Record, input: &Request) -> Result<Value> {
    let endpoint = Endpoint::for_key(&record.key)?;
    let (mut reader, mut writer) = tokio::time::timeout(Duration::from_secs(2), endpoint.connect())
        .await
        .context("shim connection timed out")??;
    write_frame(&mut writer, input).await?;
    let reply: std::result::Result<Value, ErrorReport> =
        tokio::time::timeout(Duration::from_secs(15), read_frame(&mut reader))
            .await
            .context("shim request timed out; command outcome may be unknown")??;
    reply.map_err(|report| Rejected(report.into_error()).into())
}

#[derive(Debug, thiserror::Error)]
#[error("shim rejected operation")]
struct Rejected(#[source] anyhow::Error);

async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(reader: &mut R) -> Result<T> {
    let length = reader.read_u32().await? as usize;
    ensure!(length <= 32 * 1024 * 1024, "shim frame exceeds limit");
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).context("invalid shim frame")
}
async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= 32 * 1024 * 1024, "shim frame exceeds limit");
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
fn run_directory(server: &Path) -> Result<PathBuf> {
    let path = server.join(".mcpm/run");
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&path)?;
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o077 == 0,
            "shim run directory must be private, owned by the current user and not a symlink"
        );
    }
    #[cfg(windows)]
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .with_context(|| format!("failed to open shim file {path:?}"))
}
fn lock(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    file.try_lock()
        .context("server shim is already running or launching")?;
    Ok(file)
}
fn instance_locked(directory: &Path) -> Result<bool> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("instance.lock"))
        .context("running instance lock is missing or inaccessible")?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(error)) => {
            Err(error).context("failed to inspect running instance lock")
        }
    }
}
fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match File::open(path) {
        Ok(file) => Ok(Some(
            serde_json::from_reader(file).with_context(|| format!("invalid shim data {path:?}"))?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read shim data {path:?}")),
    }
}
fn publish<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("missing shim data parent")?)?;
    serde_json::to_writer(&mut temporary, value)?;
    temporary.flush()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}
static HOST_EXECUTABLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The daemon entry supplies its actual executable, including custom --ssh-mcpmd names.
pub fn set_daemon_executable(path: PathBuf) -> Result<()> {
    HOST_EXECUTABLE
        .set(path)
        .map_err(|_| anyhow::anyhow!("shim host executable is already initialized"))
}

fn daemon_executable() -> Result<&'static Path> {
    if HOST_EXECUTABLE.get().is_none() {
        let current = std::env::current_exe()?;
        let name = if cfg!(windows) { "mcpmd.exe" } else { "mcpmd" };
        let sibling = current.with_file_name(name);
        let executable = if sibling.is_file() {
            sibling
        } else {
            PathBuf::from(name)
        };
        let _ = HOST_EXECUTABLE.set(executable);
    }
    Ok(HOST_EXECUTABLE
        .get()
        .expect("shim executable initialized")
        .as_path())
}
fn detach(command: &mut tokio::process::Command) {
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
}
