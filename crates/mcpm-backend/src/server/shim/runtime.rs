//! Java-owning shim runtime: independent control, bounded logs and serialized command outcomes.

use super::*;
use mcpm_lib::local::mc::server::launch::{JavaProcess, rcon};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::{Mutex as AsyncMutex, Notify, Semaphore, watch};

const TOKEN_LIFETIME: Duration = Duration::from_secs(30);
struct Control {
    token: String,
    holder: String,
    expires: Instant,
}
struct Data {
    state: State,
    exit_code: Option<u32>,
    channel: Channel,
    rcon_connected: bool,
    control: Option<Control>,
    stdout: logs::Buffer,
    stderr: logs::Buffer,
    io_error: Option<String>,
    output: [Option<watch::Sender<()>>; 2],
}
struct Commands {
    stdin: Option<std::process::ChildStdin>,
    settings: Option<rcon::Settings>,
    history: Option<history::History>,
    configuration_error: Option<String>,
    java_pid: u32,
    connection: Option<rcon::Connection>,
    ever_connected: bool,
    stop_attempted: bool,
}
impl Commands {
    fn history(&self) -> &history::History {
        self.history
            .as_ref()
            .expect("command history is open until Java is reaped")
    }
}
struct Runtime {
    reference: ShimRef,
    communication: Communication,
    launch_request_id: String,
    initial_token: String,
    data: Mutex<Data>,
    commands: AsyncMutex<Commands>,
    output: [watch::Receiver<()>; 2],
    status: watch::Sender<()>,
    kill: Notify,
}

pub(super) async fn start(bootstrap: Bootstrap) -> Result<()> {
    let Bootstrap {
        input,
        record,
        token,
    } = bootstrap;
    let directory = run_directory(&input.server)?;
    // This shim owns one server. Relative paths stay valid if its folder is renamed.
    std::env::set_current_dir(&directory).context("failed to enter shim run directory")?;
    let directory = PathBuf::from(".");
    let _lock = lock(&directory.join("instance.lock"))?;
    let mut listener = Endpoint::for_key(&record.key)?.listen().map_err(|error| {
        if error.is::<mcpm_exec::endpoint::AlreadyRunning>() {
            error.context(format!(
                "EndpointCollision: shim endpoint {} is already occupied; Java was not started",
                record.key
            ))
        } else {
            error
        }
    })?;
    remove_obsolete_command_databases(&directory)?;
    let _hint = RunHint::create(&record)?;
    let plan = input.plan.decode()?;
    let (settings, configuration_error) = match input.communication {
        Communication::Stdin => (None, None),
        Communication::Rcon => (rcon::Settings::from_plan(&plan)?, None),
        Communication::Auto => match rcon::Settings::from_plan(&plan) {
            Ok(settings) => (settings, None),
            Err(error) => (
                None,
                Some(format!(
                    "RCON configuration unavailable; using stdin: {error:#}"
                )),
            ),
        },
    };
    ensure!(
        input.communication != Communication::Rcon || settings.is_some(),
        "rcon mode requires configured RCON credentials"
    );
    let history = history::History::create(&directory, &record.reference.run_id)?;
    let mut java = JavaProcess::spawn(plan)?;
    let stdin = java.take_stdin().context("missing Java stdin")?;
    let stdout = java.take_stdout().context("missing Java stdout")?;
    let stderr = java.take_stderr().context("missing Java stderr")?;
    let (stdout_updates, stdout_channel) = watch::channel(());
    let (stderr_updates, stderr_channel) = watch::channel(());
    let (status, _) = watch::channel(());
    let runtime = Arc::new(Runtime {
        reference: ShimRef {
            server: PathBuf::new(),
            run_id: record.reference.run_id.clone(),
        },
        communication: input.communication,
        launch_request_id: record.launch_request_id.clone(),
        initial_token: token.clone(),
        data: Mutex::new(Data {
            state: State::Running,
            exit_code: None,
            channel: Channel::Stdin,
            rcon_connected: false,
            control: Some(Control {
                token,
                holder: "launch".into(),
                expires: Instant::now() + TOKEN_LIFETIME,
            }),
            stdout: logs::Buffer::default(),
            stderr: logs::Buffer::default(),
            io_error: None,
            output: [Some(stdout_updates), Some(stderr_updates)],
        }),
        commands: AsyncMutex::new(Commands {
            stdin: Some(stdin),
            java_pid: java.id(),
            settings,
            configuration_error,
            history: Some(history),
            connection: None,
            ever_connected: false,
            stop_attempted: false,
        }),
        output: [stdout_channel, stderr_channel],
        status,
        kill: Notify::new(),
    });
    drain(stdout, Source::Stdout, runtime.clone());
    drain(stderr, Source::Stderr, runtime.clone());
    publish(&directory.join("instance.json"), &record)?;
    let launched = runtime.launched();
    let reply: std::result::Result<Launched, ErrorReport> = Ok(launched);
    if let Err(error) = write_frame(&mut tokio::io::stdout(), &reply).await {
        eprintln!("shim readiness acknowledgement lost; Java remains managed: {error:#}");
    }
    let capacity = Arc::new(Semaphore::new(64));
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    let mut exited_at = None;
    let mut requests = tokio::task::JoinSet::new();
    if runtime.commands.lock().await.settings.is_some() {
        let runtime = runtime.clone();
        requests.spawn(async move { runtime.probe_rcon().await });
    }
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if exited_at.is_none() && let Some(exit) = java.try_wait()? {
                    let mut data = runtime.data.lock().unwrap();
                    data.state = State::Exited;
                    data.exit_code = Some(exit.shell_code());
                    data.control = None;
                    data.rcon_connected = false;
                    exited_at = Some(Instant::now());
                    runtime.status.send_replace(());
                }
                if let Some(exited) = exited_at {
                    let data = runtime.data.lock().unwrap();
                    if (data.stdout.closed && data.stderr.closed) || exited.elapsed() >= Duration::from_secs(2) { break; }
                }
            }
            _ = runtime.kill.notified(), if exited_at.is_none() => { java.kill()?; }
            _ = requests.join_next(), if !requests.is_empty() => {},
            connection = listener.accept() => {
                let (mut reader, mut writer) = connection?;
                let Ok(permit) = capacity.clone().try_acquire_owned() else { continue; };
                let runtime = runtime.clone();
                requests.spawn(async move {
                    let _permit = permit;
                    let request = tokio::time::timeout(Duration::from_secs(3), read_frame::<_, Request>(&mut reader)).await;
                    let response = match request {
                        Ok(Ok(Request::Logs(input))) => {
                            if let Err(error) = runtime.subscribe(input, &mut reader, &mut writer).await {
                                let reply: std::result::Result<LogBatch, ErrorReport> = Err(ErrorReport::capture(&error));
                                let _ = write_frame(&mut writer, &reply).await;
                            }
                            return;
                        }
                        Ok(Ok(Request::Status(input))) => {
                            if let Err(error) = runtime.subscribe_status(input, &mut reader, &mut writer).await {
                                let reply: std::result::Result<Snapshot, ErrorReport> = Err(ErrorReport::capture(&error));
                                let _ = write_frame(&mut writer, &reply).await;
                            }
                            return;
                        }
                        Ok(Ok(request)) => runtime.handle(request).await,
                        Ok(Err(error)) => Err(error),
                        Err(error) => Err(error.into()),
                    }.map_err(|error| ErrorReport::capture(&error));
                    let _ = tokio::time::timeout(Duration::from_secs(3), write_frame(&mut writer, &response)).await;
                });
            }
        }
    }
    {
        let mut data = runtime.data.lock().unwrap();
        data.stdout.closed = true;
        data.stderr.closed = true;
        data.output = [None, None];
        let receipt = Receipt {
            snapshot: runtime.snapshot_from(&data).into(),
            stdout: &data.stdout,
            stderr: &data.stderr,
        };
        publish(&directory.join("finished.json"), &receipt)?;
    }
    // Final tail lives in a bounded receipt, not an empty resident shim.
    drop(listener);
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while requests.join_next().await.is_some() {}
    })
    .await;
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    if let Some(history) = runtime.commands.lock().await.history.take() {
        history.remove()?;
    }
    remove_obsolete_command_databases(&directory)?;
    Ok(())
}

/// Only generated databases are removed, while the instance lock excludes all other shims.
fn remove_obsolete_command_databases(directory: &Path) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name.strip_prefix("commands-") else {
            continue;
        };
        let Some((id, suffix)) = rest.split_at_checked(36) else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() {
            continue;
        }
        if suffix.is_empty() && entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())
                .context("failed to remove completed command history")?;
        } else if [".sqlite", ".sqlite-journal", ".sqlite-wal", ".sqlite-shm"].contains(&suffix) {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("failed to remove completed command database {name}"))?;
        }
    }
    Ok(())
}

fn drain(mut source: impl Read + Send + 'static, kind: Source, runtime: Arc<Runtime>) {
    std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        loop {
            match source.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => {
                    let mut data = runtime.data.lock().unwrap();
                    match kind {
                        Source::Stdout => &mut data.stdout,
                        Source::Stderr => &mut data.stderr,
                    }
                    .push(bytes[..length].to_vec());
                    if let Some(sender) = &data.output[usize::from(kind == Source::Stderr)] {
                        sender.send_replace(());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    runtime.data.lock().unwrap().io_error =
                        Some(format!("failed to read Java {kind:?}: {error}"));
                    break;
                }
            }
        }
        let mut data = runtime.data.lock().unwrap();
        match kind {
            Source::Stdout => &mut data.stdout,
            Source::Stderr => &mut data.stderr,
        }
        .closed = true;
        data.output[usize::from(kind == Source::Stderr)] = None;
    });
}

impl Runtime {
    async fn subscribe<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &self,
        mut input: LogsInput,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<()> {
        ensure!(
            input.reference.run_id == self.reference.run_id,
            "EndpointCollision: refusing logs for a different run"
        );
        let mut channel = self.output[usize::from(input.source == Source::Stderr)].clone();
        channel.borrow_and_update();
        loop {
            let (batch, latest) = {
                let data = self.data.lock().unwrap();
                if let Some(error) = &data.io_error {
                    bail!("{error}");
                }
                let buffer = match input.source {
                    Source::Stdout => &data.stdout,
                    Source::Stderr => &data.stderr,
                };
                (buffer.read(&input)?, buffer.range().latest)
            };
            let finished = batch.finished;
            input.cursor = Some(batch.next_cursor);
            if !batch.batch.is_empty() || batch.gap.is_some() || finished {
                let reply: std::result::Result<LogBatch, ErrorReport> = Ok(batch);
                tokio::select! {
                    _ = reader.read_u8() => return Ok(()),
                    sent = write_frame(writer, &reply) => sent?,
                }
            }
            if finished {
                return Ok(());
            }
            if input.cursor.is_some_and(|cursor| cursor < latest) {
                continue;
            }
            tokio::select! {
                _ = reader.read_u8() => return Ok(()),
                _ = channel.changed() => {},
            }
        }
    }
    async fn subscribe_status<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &self,
        input: StatusInput,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<()> {
        ensure!(
            input.reference.run_id == self.reference.run_id,
            "EndpointCollision: refusing status for a different run"
        );
        // Register before taking the initial snapshot, so no transition can fall between them.
        let mut changed = self.status.subscribe();
        changed.borrow_and_update();
        let mut heartbeat = tokio::time::interval_at(
            tokio::time::Instant::now() + input.interval(),
            input.interval(),
        );
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let snapshot = self.snapshot_from(&self.data.lock().unwrap());
            let exited = snapshot.state == State::Exited;
            let reply: std::result::Result<Snapshot, ErrorReport> = Ok(snapshot);
            tokio::select! {
                _ = reader.read_u8() => return Ok(()),
                sent = tokio::time::timeout(input.timeout(), write_frame(writer, &reply)) => {
                    sent.context("state Channel write timed out")??;
                }
            }
            if exited {
                return Ok(());
            }
            tokio::select! {
                _ = reader.read_u8() => return Ok(()),
                update = changed.changed() => { update.context("state notification Channel closed before Java exit")?; },
                _ = heartbeat.tick() => {},
            }
        }
    }
    fn launched(&self) -> Launched {
        Launched {
            reference: self.reference.clone(),
            control_token: self.initial_token.clone(),
            initial_cursor: 0,
            state: self.data.lock().unwrap().state,
        }
    }
    fn snapshot_from(&self, data: &Data) -> Snapshot {
        Snapshot {
            reference: self.reference.clone(),
            state: data.state,
            exit_code: data.exit_code,
            communication: self.communication,
            actual_channel: data.channel,
            server_status: if data.rcon_connected && data.state != State::Exited {
                ServerStatus::RconConnected
            } else {
                ServerStatus::Unknown
            },
            control_holder: data
                .control
                .as_ref()
                .filter(|control| control.expires > Instant::now())
                .map(|control| control.holder.clone()),
            stdout: data.stdout.range(),
            stderr: data.stderr.range(),
        }
    }
    fn authorize(data: &mut Data, token: &str) -> bool {
        if let Some(control) = &mut data.control
            && control.expires > Instant::now()
            && control.token == token
        {
            control.expires = Instant::now() + TOKEN_LIFETIME;
            true
        } else {
            false
        }
    }
    fn connected(&self, connected: bool) {
        let mut data = self.data.lock().unwrap();
        let connected = connected && matches!(data.state, State::Running | State::Stopping);
        if data.rcon_connected != connected {
            data.rcon_connected = connected;
            self.status.send_replace(());
        }
    }

    /// One serialized authentication attempt at a time; never restart initial discovery.
    async fn probe_rcon(&self) {
        loop {
            {
                let mut lane = self.commands.lock().await;
                if self.data.lock().unwrap().state == State::Exited {
                    return;
                }
                if lane.stop_attempted {
                    self.connected(false);
                    return;
                }
                if let Some(connection) = &lane.connection {
                    if !connection.is_connected() {
                        lane.connection = None;
                        self.connected(false);
                    }
                } else if !lane.ever_connected
                    && self.data.lock().unwrap().state == State::Running
                    && let Some(settings) = &lane.settings
                    && let Ok(connection) =
                        rcon::Connection::connect_owned(settings, lane.java_pid).await
                {
                    lane.connection = Some(connection);
                    lane.ever_connected = true;
                    self.connected(true);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    async fn handle(&self, request: Request) -> Result<Value> {
        ensure!(
            request.run_id() == Some(self.reference.run_id.as_str()),
            "EndpointCollision: endpoint belongs to a different run; refusing request"
        );
        match request {
            Request::LaunchResult { request_id, .. } => {
                ensure!(
                    request_id == self.launch_request_id,
                    "AlreadyRunning: this server has an active Java; use attach"
                );
                Ok(serde_json::to_value(self.launched())?)
            }
            Request::Inspect(_) => Ok(serde_json::to_value(
                self.snapshot_from(&self.data.lock().unwrap()),
            )?),
            Request::Acquire(input) => {
                ensure!(
                    !input.requester.is_empty() && input.requester.len() <= 128,
                    "invalid control requester"
                );
                let mut data = self.data.lock().unwrap();
                ensure!(data.state != State::Exited, "Java has exited");
                let result = if let Some(control) = &data.control
                    && control.expires > Instant::now()
                {
                    AcquireResult::Busy {
                        current_holder: control.holder.clone(),
                    }
                } else {
                    let token = uuid::Uuid::new_v4().to_string();
                    data.control = Some(Control {
                        token: token.clone(),
                        holder: input.requester,
                        expires: Instant::now() + TOKEN_LIFETIME,
                    });
                    self.status.send_replace(());
                    AcquireResult::Acquired { token }
                };
                Ok(serde_json::to_value(result)?)
            }
            Request::Release(input) => {
                let mut data = self.data.lock().unwrap();
                let result = if data.control.is_none() {
                    ReleaseResult::AlreadyReleased
                } else if Self::authorize(&mut data, &input.control_token) {
                    data.control = None;
                    self.status.send_replace(());
                    ReleaseResult::Released
                } else {
                    ReleaseResult::InvalidToken
                };
                Ok(serde_json::to_value(result)?)
            }
            Request::Kill(input) => {
                let mut data = self.data.lock().unwrap();
                let result = if data.state == State::Exited {
                    KillResult::AlreadyExited
                } else if !Self::authorize(&mut data, &input.control_token) {
                    KillResult::InvalidToken
                } else {
                    if data.state != State::Killing {
                        data.state = State::Killing;
                        self.status.send_replace(());
                    }
                    self.kill.notify_one();
                    KillResult::Accepted
                };
                Ok(serde_json::to_value(result)?)
            }
            Request::Logs(_) => bail!("log subscription requires a dedicated channel"),
            Request::Status(_) => bail!("state subscription requires a dedicated channel"),
            Request::Command(input) => Ok(serde_json::to_value(self.command(input).await?)?),
        }
    }
    async fn command(&self, input: CommandInput) -> Result<CommandResult> {
        ensure!(
            !input.command_id.is_empty() && input.command_id.len() <= 128,
            "invalid command id"
        );
        ensure!(
            input.text.len() <= 16 * 1024 && !input.text.contains(['\0', '\n', '\r']),
            "command must be one line of at most 16 KiB"
        );
        {
            let mut data = self.data.lock().unwrap();
            let status = if data.state == State::Exited {
                CommandStatus::Exited
            } else if !Self::authorize(&mut data, &input.control_token) {
                CommandStatus::InvalidToken
            } else {
                CommandStatus::Accepted
            };
            if status != CommandStatus::Accepted {
                return Ok(CommandResult {
                    status,
                    channel: data.channel,
                    output: None,
                    result: CommandOutcome::Unknown,
                    diagnostic: None,
                });
            }
        }
        let mut lane = self.commands.lock().await;
        let digest = blake3::hash(input.text.as_bytes()).to_hex().to_string();
        // Recheck control after queueing; a stale queued caller must not execute after takeover.
        {
            let mut data = self.data.lock().unwrap();
            if data.state == State::Exited || !Self::authorize(&mut data, &input.control_token) {
                return Ok(CommandResult {
                    status: if data.state == State::Exited {
                        CommandStatus::Exited
                    } else {
                        CommandStatus::InvalidToken
                    },
                    channel: data.channel,
                    output: None,
                    result: CommandOutcome::Unknown,
                    diagnostic: None,
                });
            }
        }
        let mut result = CommandResult {
            status: CommandStatus::Accepted,
            channel: Channel::Stdin,
            output: None,
            result: CommandOutcome::Unknown,
            diagnostic: lane.configuration_error.clone(),
        };
        let ticket = match lane.history().prepare(input.command_id, digest, &result)? {
            history::Prepared::Existing(previous) => return Ok(previous),
            history::Prepared::Pending(ticket) => ticket,
        };
        let command = input.text.trim();
        let is_stop = command.strip_prefix('/').unwrap_or(command) == "stop";
        if is_stop {
            lane.stop_attempted = true;
            self.connected(false);
            let mut data = self.data.lock().unwrap();
            if data.state == State::Running {
                data.state = State::Stopping;
                self.status.send_replace(());
            }
        }
        let attempt = match lane
            .connection
            .take()
            .filter(rcon::Connection::is_connected)
        {
            Some(connection) => Some(Ok(connection)),
            None => match &lane.settings {
                Some(settings) => {
                    Some(rcon::Connection::connect_owned(settings, lane.java_pid).await)
                }
                None => None,
            },
        };
        match attempt {
            Some(Ok(mut connection)) => {
                lane.ever_connected = true;
                self.connected(!lane.stop_attempted);
                result.channel = Channel::Rcon;
                {
                    let mut data = self.data.lock().unwrap();
                    if data.channel != Channel::Rcon {
                        data.channel = Channel::Rcon;
                        self.status.send_replace(());
                    }
                }
                match connection.execute(&input.text).await {
                    Ok(output) => {
                        result.output = Some(output);
                        result.result = CommandOutcome::Completed;
                        lane.connection = Some(connection);
                    }
                    Err(error) => {
                        self.connected(false);
                        result.diagnostic = Some(format!(
                            "RCON outcome unknown; command was not replayed: {error:#}"
                        ));
                    }
                }
            }
            connect => {
                self.connected(false);
                if let Some(Err(error)) = connect {
                    result.diagnostic = Some(format!("RCON unavailable: {error:#}"));
                }
                {
                    let mut data = self.data.lock().unwrap();
                    if data.channel != Channel::Stdin {
                        data.channel = Channel::Stdin;
                        self.status.send_replace(());
                    }
                }
                let stdin = lane.stdin.take();
                let text = input.text;
                let (stdin, sent) = tokio::task::spawn_blocking(move || {
                    let Some(mut stdin) = stdin else {
                        return (None, Err(anyhow::anyhow!("Java stdin is closed")));
                    };
                    let sent = (|| -> Result<()> {
                        stdin.write_all(text.as_bytes())?;
                        stdin.write_all(b"\n")?;
                        stdin.flush()?;
                        Ok(())
                    })();
                    (Some(stdin), sent)
                })
                .await?;
                lane.stdin = stdin;
                match sent {
                    Ok(()) => result.result = CommandOutcome::Sent,
                    Err(error) => {
                        result.diagnostic = Some(format!(
                            "stdin outcome unknown; command was not replayed: {error:#}"
                        ))
                    }
                }
            }
        }
        lane.history
            .as_mut()
            .expect("active command history")
            .complete(ticket, &result)?;
        Ok(result)
    }
}
