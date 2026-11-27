//! Java-owning shim runtime: independent control, bounded logs and serialized command outcomes.

use super::*;
use mcpm_lib::local::mc::{
    log::{ServerStatusEvent, ServerStatusScanner},
    server::{
        launch::{JavaProcess, rcon},
        restart::{
            CompiledRestartPolicy, ExitObservation, ObservedCrashReport, RestartDecision,
            RestartHistory, current_local_timestamp,
        },
    },
};
use std::{
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime},
};
use tokio::sync::{Mutex as AsyncMutex, Notify, Semaphore, watch};

struct Data {
    generation: u64,
    state: State,
    reaped: bool,
    kill_requested: bool,
    restart_cancelled: bool,
    has_restart_policy: bool,
    restart_deadline: Option<tokio::time::Instant>,
    channel: Channel,
    logs: logs::Buffer,
    io_error: Option<String>,
    events: events::Events,
    output: Option<watch::Sender<()>>,
}
struct Commands {
    stdin: Option<std::process::ChildStdin>,
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
    rcon_settings: Option<rcon::Settings>,
    launch_request_id: String,
    data: Mutex<Data>,
    commands: AsyncMutex<Commands>,
    output: watch::Receiver<()>,
    status: watch::Sender<()>,
    command_updates: watch::Sender<()>,
    kill: Notify,
    cancel_restart: Notify,
    pipe_closed: Notify,
}

enum SupervisionPhase {
    Running {
        java: JavaProcess,
        started_at: Instant,
        started_wall: SystemTime,
        logs_since: u64,
        stdout_drained: Arc<AtomicBool>,
        stderr_drained: Arc<AtomicBool>,
        killed_by_mcpm: bool,
        pending_exit: Option<(LaunchExit, bool, Instant)>,
    },
    Restarting {
        previous_exit: ExitReason,
        deadline: tokio::time::Instant,
    },
}

pub(super) async fn start(bootstrap: Bootstrap) -> Result<()> {
    let Bootstrap { input, record } = bootstrap;
    let directory = run_directory(&input.server)?;
    // This shim owns one server. Relative paths stay valid if its folder is renamed.
    std::env::set_current_dir(&directory).context("failed to enter shim run directory")?;
    let directory = PathBuf::from(".");
    let _lock = lock(&directory.join(RUNNING_LOCK))?;
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
    let previous = read_receipt(&directory)?;
    logs::remove_obsolete(
        &directory,
        previous
            .as_ref()
            .map(|receipt| receipt.snapshot.reference.run_id.as_str()),
    )?;
    let _hint = RunHint::create(&record)?;
    let plan = input.plan.decode()?;
    let restart_policy =
        CompiledRestartPolicy::compile(&input.restart).context("invalid restart_on policy")?;
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
    let initial_rcon_status = if settings.is_some() {
        RconStatus::Unknown
    } else {
        RconStatus::Unavailable
    };
    let has_restart_policy = !restart_policy.is_empty();
    let history = history::History::create(&directory, &record.reference.run_id)?;
    let java_directory = plan.directory().to_path_buf();
    let started_wall = SystemTime::now();
    let mut java = JavaProcess::spawn(plan.clone())
        .map_err(crate::operations::server::shim::attach_executable_tip)?;
    let stdin = java.take_stdin().context("missing Java stdin")?;
    let stdout = java.take_stdout().context("missing Java stdout")?;
    let stderr = java.take_stderr().context("missing Java stderr")?;
    let (log_updates, log_channel) = watch::channel(());
    let (status, _) = watch::channel(());
    let (command_updates, _) = watch::channel(());
    let runtime = Arc::new(Runtime {
        reference: ShimRef {
            server: PathBuf::new(),
            run_id: record.reference.run_id.clone(),
        },
        communication: input.communication,
        rcon_settings: settings,
        launch_request_id: record.launch_request_id.clone(),
        data: Mutex::new(Data {
            generation: 0,
            state: State::Running {
                server_status: ServerStatus::StartingOrUnknown,
                rcon_status: initial_rcon_status,
            },
            reaped: false,
            kill_requested: false,
            restart_cancelled: false,
            has_restart_policy,
            restart_deadline: None,
            channel: Channel::Stdin,
            logs: logs::Buffer::default(),
            io_error: None,
            output: Some(log_updates),
            events: events::Events::default(),
        }),
        commands: AsyncMutex::new(Commands {
            stdin: Some(stdin),
            java_pid: java.id(),
            configuration_error,
            history: Some(history),
            connection: None,
            ever_connected: false,
            stop_attempted: false,
        }),
        output: log_channel,
        status,
        command_updates,
        kill: Notify::new(),
        cancel_restart: Notify::new(),
        pipe_closed: Notify::new(),
    });
    let stdout_drained = Arc::new(AtomicBool::new(false));
    let stderr_drained = Arc::new(AtomicBool::new(false));
    let startup_done = Arc::new(AtomicBool::new(false));
    drain(
        stdout,
        Source::Stdout,
        0,
        runtime.clone(),
        stdout_drained.clone(),
        startup_done.clone(),
    );
    drain(
        stderr,
        Source::Stderr,
        0,
        runtime.clone(),
        stderr_drained.clone(),
        startup_done,
    );
    publish(&directory.join("instance.json"), &record)?;
    let launched = runtime.launched();
    let reply: std::result::Result<Launched, ErrorReport> = Ok(launched);
    if let Err(error) = write_frame(&mut tokio::io::stdout(), &reply).await {
        eprintln!("shim readiness acknowledgement lost; Java remains managed: {error:#}");
    }
    let capacity = Arc::new(Semaphore::new(64));
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    let mut requests = tokio::task::JoinSet::new();
    let mut rcon_probe: Option<tokio::task::JoinHandle<()>> =
        runtime.rcon_settings.is_some().then(|| {
            let runtime = runtime.clone();
            tokio::spawn(async move { runtime.probe_rcon().await })
        });
    let mut restart_history = RestartHistory::new();
    let mut phase = SupervisionPhase::Running {
        java,
        started_at: Instant::now(),
        started_wall,
        logs_since: 0,
        stdout_drained,
        stderr_drained,
        killed_by_mcpm: false,
        pending_exit: None,
    };

    loop {
        match &mut phase {
            SupervisionPhase::Running { .. } => {
                let mut should_check_exit = false;
                let mut should_kill = false;
                tokio::select! {
                    _ = tick.tick() => {
                        should_check_exit = true;
                    }
                    _ = runtime.pipe_closed.notified() => {
                        should_check_exit = true;
                    }
                    _ = runtime.kill.notified(), if matches!(phase, SupervisionPhase::Running { pending_exit: None, .. }) => {
                        should_kill = true;
                    }
                    _ = requests.join_next(), if !requests.is_empty() => {},
                    connection = listener.accept() => {
                        accept_connection(connection?, &capacity, &runtime, &mut requests);
                    }
                }

                if should_kill
                    && runtime.data.lock().unwrap().kill_requested
                    && let SupervisionPhase::Running {
                        java,
                        killed_by_mcpm,
                        pending_exit,
                        ..
                    } = &mut phase
                    && pending_exit.is_none()
                {
                    if let Some(exit) = java.try_wait()? {
                        if let Some(handle) = rcon_probe.take() {
                            handle.abort();
                        }
                        *pending_exit = Some(mark_reaped(&runtime, exit, *killed_by_mcpm));
                        should_check_exit = true;
                    } else {
                        java.kill()?;
                        *killed_by_mcpm = true;
                    }
                }

                if should_check_exit {
                    let finalized = if let SupervisionPhase::Running {
                        java,
                        started_at,
                        started_wall,
                        logs_since,
                        stdout_drained,
                        stderr_drained,
                        killed_by_mcpm,
                        pending_exit,
                    } = &mut phase
                    {
                        if pending_exit.is_none()
                            && let Some(exit) = java.try_wait()?
                        {
                            if let Some(handle) = rcon_probe.take() {
                                handle.abort();
                            }
                            *pending_exit = Some(mark_reaped(&runtime, exit, *killed_by_mcpm));
                        }
                        if let Some((exit, killed, exited_at)) = *pending_exit {
                            let pipes_ready = (stdout_drained.load(Ordering::Acquire)
                                && stderr_drained.load(Ordering::Acquire))
                                || exited_at.elapsed() >= Duration::from_secs(2);
                            if pipes_ready {
                                Some((
                                    exit,
                                    killed,
                                    exited_at,
                                    *started_at,
                                    *started_wall,
                                    *logs_since,
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some((exit, killed, exited_at, started_at, started_wall, logs_since)) =
                        finalized
                    {
                        let candidates = {
                            let data = runtime.data.lock().unwrap();
                            logs::detect_crash_report_candidates(
                                &data.logs,
                                &java_directory,
                                logs_since,
                            )
                        };
                        let mut observed_report = None;
                        let detected_report =
                            verify_crash_report(&candidates, &java_directory, started_wall).map(
                                |(path, analysis)| {
                                    observed_report = Some(ObservedCrashReport {
                                        path: path.clone(),
                                        analysis: Ok(analysis.clone()),
                                    });
                                    DetectedCrashReport {
                                        path,
                                        analysis: Ok(analysis),
                                    }
                                },
                            );
                        let exit_reason =
                            ExitReason::from_launch_exit(exit, killed, detected_report);

                        let (stop_requested, rcon_ever_connected) = {
                            let mut lane = runtime.commands.lock().await;
                            lane.stdin = None;
                            lane.connection = None;
                            (lane.stop_attempted, lane.ever_connected)
                        };

                        let observation = ExitObservation {
                            exit,
                            killed_by_mcpm: killed,
                            stop_requested,
                            crash_report: observed_report,
                            uptime: exited_at.saturating_duration_since(started_at),
                            rcon_ever_connected,
                            now: current_local_timestamp(),
                            at: exited_at,
                        };
                        let outcome = restart_policy.evaluate(&observation, &mut restart_history);
                        for message in &outcome.diagnostics {
                            tracing::warn!("{message}");
                        }

                        match outcome.decision {
                            RestartDecision::Stop { .. } => {
                                let mut data = runtime.data.lock().unwrap();
                                data.restart_deadline = None;
                                data.state = State::Exited {
                                    reason: exit_reason,
                                };
                                runtime.status.send_replace(());
                                break;
                            }
                            RestartDecision::Restart {
                                rule,
                                delay,
                                attempt,
                            } => {
                                let delay_ms =
                                    u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
                                let deadline = tokio::time::Instant::now()
                                    .checked_add(delay)
                                    .unwrap_or_else(|| {
                                        tokio::time::Instant::now()
                                            + mcpm_lib::local::mc::server::restart::MAX_RESTART_DURATION
                                    });
                                let suppressed = {
                                    let mut data = runtime.data.lock().unwrap();
                                    if data.kill_requested || data.restart_cancelled {
                                        data.restart_deadline = None;
                                        data.state = State::Exited {
                                            reason: exit_reason.clone(),
                                        };
                                        runtime.status.send_replace(());
                                        true
                                    } else {
                                        data.restart_deadline = Some(deadline);
                                        data.state = State::Restarting {
                                            previous_exit: exit_reason.clone(),
                                            rule: rule.clone(),
                                            attempt,
                                            delay_ms,
                                            remaining_ms: delay_ms,
                                        };
                                        runtime.status.send_replace(());
                                        false
                                    }
                                };
                                if suppressed {
                                    break;
                                }
                                tracing::info!(
                                    rule = %rule,
                                    attempt,
                                    delay_ms,
                                    "scheduling server auto-restart"
                                );
                                phase = SupervisionPhase::Restarting {
                                    previous_exit: exit_reason,
                                    deadline,
                                };
                            }
                        }
                    }
                }
            }
            SupervisionPhase::Restarting { deadline, .. } => {
                let deadline = *deadline;
                let mut due = false;
                let mut cancelled = false;
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => {
                        due = true;
                    }
                    _ = runtime.cancel_restart.notified() => {
                        cancelled = true;
                    }
                    _ = requests.join_next(), if !requests.is_empty() => {},
                    connection = listener.accept() => {
                        accept_connection(connection?, &capacity, &runtime, &mut requests);
                    }
                }

                let cancelled =
                    cancelled || runtime.data.lock().unwrap().restart_cancelled;
                if cancelled {
                    let SupervisionPhase::Restarting { previous_exit, .. } = phase else {
                        unreachable!();
                    };
                    let mut data = runtime.data.lock().unwrap();
                    data.restart_deadline = None;
                    data.state = State::Exited {
                        reason: previous_exit,
                    };
                    runtime.status.send_replace(());
                    break;
                }

                if due {
                    let SupervisionPhase::Restarting { previous_exit, .. } = phase else {
                        unreachable!();
                    };
                    let started_wall = SystemTime::now();
                    match spawn_next_generation(&plan, &runtime).await {
                        Ok((mut next_java, stdout, stderr)) => {
                            let stdout_drained = Arc::new(AtomicBool::new(false));
                            let stderr_drained = Arc::new(AtomicBool::new(false));
                            let startup_done = Arc::new(AtomicBool::new(false));
                            let respawned = {
                                let mut data = runtime.data.lock().unwrap();
                                if data.restart_cancelled {
                                    data.restart_deadline = None;
                                    data.state = State::Exited {
                                        reason: previous_exit.clone(),
                                    };
                                    runtime.status.send_replace(());
                                    None
                                } else {
                                    let logs_since = data.logs.range().latest;
                                    data.generation = data.generation.saturating_add(1);
                                    let generation = data.generation;
                                    data.reaped = false;
                                    data.kill_requested = false;
                                    data.restart_cancelled = false;
                                    data.restart_deadline = None;
                                    data.channel = Channel::Stdin;
                                    data.state = State::Running {
                                        server_status: ServerStatus::StartingOrUnknown,
                                        rcon_status: runtime.disconnected_rcon_status(),
                                    };
                                    runtime.status.send_replace(());
                                    Some((logs_since, generation))
                                }
                            };
                            let Some((logs_since, generation)) = respawned else {
                                let _ = next_java.kill();
                                let mut lane = runtime.commands.lock().await;
                                lane.stdin = None;
                                lane.connection = None;
                                break;
                            };
                            drain(
                                stdout,
                                Source::Stdout,
                                generation,
                                runtime.clone(),
                                stdout_drained.clone(),
                                startup_done.clone(),
                            );
                            drain(
                                stderr,
                                Source::Stderr,
                                generation,
                                runtime.clone(),
                                stderr_drained.clone(),
                                startup_done,
                            );
                            if runtime.rcon_settings.is_some() {
                                let runtime = runtime.clone();
                                rcon_probe =
                                    Some(tokio::spawn(async move { runtime.probe_rcon().await }));
                            }
                            phase = SupervisionPhase::Running {
                                java: next_java,
                                started_at: Instant::now(),
                                started_wall,
                                logs_since,
                                stdout_drained,
                                stderr_drained,
                                killed_by_mcpm: false,
                                pending_exit: None,
                            };
                        }
                        Err(error) => {
                            tracing::error!("failed to respawn Java during auto-restart: {error:#}");
                            let mut data = runtime.data.lock().unwrap();
                            data.logs.push(
                                Source::Stderr,
                                format!("mcpm: failed to respawn Java during auto-restart: {error:#}\n")
                                    .into_bytes(),
                            );
                            if let Some(sender) = &data.output {
                                sender.send_replace(());
                            }
                            data.state = State::Exited {
                                reason: previous_exit,
                            };
                            runtime.status.send_replace(());
                            break;
                        }
                    }
                }
            }
        }
    }
    if let Some(handle) = rcon_probe.take() {
        handle.abort();
    }
    // Finish an in-flight exchange before closing the shared result Channel.
    {
        let _lane = runtime.commands.lock().await;
        runtime.data.lock().unwrap().events.closed = true;
        runtime.command_updates.send_replace(());
    }
    {
        let mut data = runtime.data.lock().unwrap();
        data.logs.closed = true;
        data.output = None;
        data.logs.persist(&directory, &record.reference.run_id)?;
        let receipt = Receipt {
            version: RECEIPT_VERSION,
            snapshot: runtime.snapshot_from(&data).into(),
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

/// Only generated databases are removed, while the running lock excludes all other shims.
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

fn mark_reaped(runtime: &Runtime, exit: LaunchExit, killed: bool) -> (LaunchExit, bool, Instant) {
    let mut data = runtime.data.lock().unwrap();
    data.reaped = true;
    let disconnected = runtime.disconnected_rcon_status();
    match &mut data.state {
        State::Running { rcon_status, .. } | State::Stopping { rcon_status, .. } => {
            *rcon_status = disconnected;
        }
        State::Killing | State::Restarting { .. } | State::Exited { .. } => {}
    }
    (exit, killed, Instant::now())
}

async fn spawn_next_generation(
    plan: &mcpm_lib::local::mc::server::launch::LaunchPlan,
    runtime: &Runtime,
) -> Result<(
    JavaProcess,
    std::process::ChildStdout,
    std::process::ChildStderr,
)> {
    let mut java = JavaProcess::spawn(plan.clone())
        .map_err(crate::operations::server::shim::attach_executable_tip)?;
    let stdin = java.take_stdin().context("missing Java stdin")?;
    let stdout = java.take_stdout().context("missing Java stdout")?;
    let stderr = java.take_stderr().context("missing Java stderr")?;
    let mut lane = runtime.commands.lock().await;
    lane.stdin = Some(stdin);
    lane.java_pid = java.id();
    lane.connection = None;
    lane.ever_connected = false;
    lane.stop_attempted = false;
    Ok((java, stdout, stderr))
}

fn accept_connection<R, W>(
    (mut reader, mut writer): (R, W),
    capacity: &Arc<Semaphore>,
    runtime: &Arc<Runtime>,
    requests: &mut tokio::task::JoinSet<()>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let Ok(permit) = capacity.clone().try_acquire_owned() else {
        return;
    };
    let runtime = runtime.clone();
    requests.spawn(async move {
        let _permit = permit;
        let request = tokio::time::timeout(
            Duration::from_secs(3),
            read_frame::<_, Request>(&mut reader),
        )
        .await;
        let response = match request {
            Ok(Ok(Request::Logs(input))) => {
                if let Err(error) = runtime.subscribe(input, &mut reader, &mut writer).await {
                    let reply: std::result::Result<LogBatch, ErrorReport> =
                        Err(ErrorReport::capture(&error));
                    let _ = write_frame(&mut writer, &reply).await;
                }
                return;
            }
            Ok(Ok(Request::Commands(input))) => {
                if let Err(error) = runtime
                    .subscribe_commands(input, &mut reader, &mut writer)
                    .await
                {
                    let reply: std::result::Result<CommandEvent, ErrorReport> =
                        Err(ErrorReport::capture(&error));
                    let _ = write_frame(&mut writer, &reply).await;
                }
                return;
            }
            Ok(Ok(Request::Status(input))) => {
                if let Err(error) = runtime
                    .subscribe_status(input, &mut reader, &mut writer)
                    .await
                {
                    let reply: std::result::Result<Snapshot, ErrorReport> =
                        Err(ErrorReport::capture(&error));
                    let _ = write_frame(&mut writer, &reply).await;
                }
                return;
            }
            Ok(Ok(request)) => runtime.handle(request).await,
            Ok(Err(error)) => Err(error),
            Err(error) => Err(error.into()),
        }
        .map_err(|error| ErrorReport::capture(&error));
        let _ =
            tokio::time::timeout(Duration::from_secs(3), write_frame(&mut writer, &response)).await;
    });
}

fn drain(
    mut source: impl Read + Send + 'static,
    kind: Source,
    generation: u64,
    runtime: Arc<Runtime>,
    drained: Arc<AtomicBool>,
    startup_done: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut bytes = vec![0; MAX_LOG_BLOCK_BYTES];
        let mut scanner = ServerStatusScanner::default();
        loop {
            match source.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => {
                    let event = if !startup_done.load(Ordering::Acquire) {
                        scanner.feed(&bytes[..length])
                    } else {
                        None
                    };
                    if event == Some(ServerStatusEvent::Done) {
                        startup_done.store(true, Ordering::Release);
                    }
                    let mut status_changed = false;
                    {
                        let mut data = runtime.data.lock().unwrap();
                        data.logs.push(kind, bytes[..length].to_vec());
                        if let Some(sender) = &data.output {
                            sender.send_replace(());
                        }
                        if let Some(event) = event
                            && data.generation == generation
                            && !data.reaped
                        {
                            let next = match event {
                                ServerStatusEvent::GeneratingChunks(progress) => {
                                    ServerStatus::GeneratingChunks(progress)
                                }
                                ServerStatusEvent::Done => ServerStatus::Done,
                            };
                            match &mut data.state {
                                State::Running { server_status, .. }
                                | State::Stopping { server_status, .. }
                                    if *server_status != ServerStatus::Done
                                        && *server_status != next =>
                                {
                                    *server_status = next;
                                    status_changed = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    if status_changed {
                        runtime.status.send_replace(());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let mut data = runtime.data.lock().unwrap();
                    data.io_error = Some(format!("failed to read Java {kind:?}: {error}"));
                    if let Some(sender) = &data.output {
                        sender.send_replace(());
                    }
                    break;
                }
            }
        }
        drained.store(true, Ordering::Release);
        runtime.pipe_closed.notify_one();
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
        let mut channel = self.output.clone();
        channel.borrow_and_update();
        loop {
            let (batch, latest) = {
                let data = self.data.lock().unwrap();
                if let Some(error) = &data.io_error {
                    bail!("{error}");
                }
                (data.logs.read(&input)?, data.logs.range().latest)
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
            let exited = snapshot.state.is_exited();
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
    async fn subscribe_commands<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &self,
        input: CommandsInput,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<()> {
        ensure!(
            input.reference.run_id == self.reference.run_id,
            "command results belong to a different run"
        );
        let mut updates = self.command_updates.subscribe();
        let mut cursor = self.data.lock().unwrap().events.next;
        let ready: std::result::Result<CommandEvent, ErrorReport> = Ok(CommandEvent::Ready);
        write_frame(writer, &ready).await?;
        loop {
            updates.borrow_and_update();
            let event = self.data.lock().unwrap().events.read(&mut cursor);
            if let Some(event) = event {
                let done = matches!(&*event, CommandEvent::Done);
                let reply: std::result::Result<&CommandEvent, ErrorReport> = Ok(&event);
                tokio::select! {
                    _ = reader.read_u8() => return Ok(()),
                    sent = write_frame(writer, &reply) => sent?,
                }
                if done {
                    return Ok(());
                }
            } else {
                tokio::select! {
                    _ = reader.read_u8() => return Ok(()),
                    changed = updates.changed() => { changed?; },
                }
            }
        }
    }

    fn command_event(
        &self,
        listener_id: String,
        command_id: String,
        text: String,
        result: CommandResult,
    ) {
        self.data
            .lock()
            .unwrap()
            .events
            .push(CommandEvent::Completed {
                listener_id,
                command_id,
                text,
                result,
            });
        self.command_updates.send_replace(());
    }
    fn launched(&self) -> Launched {
        Launched {
            reference: self.reference.clone(),
            initial_cursor: 0,
            state: self.data.lock().unwrap().state.clone(),
        }
    }
    fn snapshot_from(&self, data: &Data) -> Snapshot {
        let mut state = data.state.clone();
        if let State::Restarting { remaining_ms, .. } = &mut state
            && let Some(deadline) = data.restart_deadline
        {
            let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
            *remaining_ms = u64::try_from(rem.as_millis()).unwrap_or(u64::MAX);
        }
        Snapshot {
            reference: self.reference.clone(),
            state,
            communication: self.communication,
            actual_channel: data.channel,
            logs: data.logs.range(),
        }
    }
    fn disconnected_rcon_status(&self) -> RconStatus {
        if self.rcon_settings.is_some() {
            RconStatus::Unknown
        } else {
            RconStatus::Unavailable
        }
    }

    fn connected(&self, connected: bool) {
        let mut data = self.data.lock().unwrap();
        let target = if connected && !data.reaped && self.rcon_settings.is_some() {
            RconStatus::Connected
        } else {
            self.disconnected_rcon_status()
        };
        match &mut data.state {
            State::Running { rcon_status, .. } | State::Stopping { rcon_status, .. }
                if *rcon_status != target =>
            {
                *rcon_status = target;
                self.status.send_replace(());
            }
            _ => {}
        }
    }

    /// One serialized authentication attempt at a time; never restart initial discovery.
    async fn probe_rcon(&self) {
        loop {
            {
                let mut lane = self.commands.lock().await;
                {
                    let data = self.data.lock().unwrap();
                    if data.state.is_exited() || data.reaped {
                        return;
                    }
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
                    && {
                        let data = self.data.lock().unwrap();
                        matches!(data.state, State::Running { .. }) && !data.reaped
                    }
                    && let Some(settings) = &self.rcon_settings
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
            Request::Kill(input) => {
                ensure!(
                    uuid::Uuid::parse_str(&input.listener_id).is_ok(),
                    "invalid Listener identity"
                );
                ensure!(
                    !input.command_id.is_empty() && input.command_id.len() <= 128,
                    "invalid command id"
                );
                let mut data = self.data.lock().unwrap();
                let channel = data.channel;
                let result = if data.state.is_exited() || data.restart_cancelled {
                    KillResult::AlreadyExited
                } else if matches!(data.state, State::Restarting { .. })
                    || (data.reaped && data.has_restart_policy)
                {
                    data.restart_cancelled = true;
                    data.events.push(CommandEvent::Completed {
                        listener_id: input.listener_id,
                        command_id: input.command_id,
                        text: "@kill".into(),
                        result: CommandResult {
                            status: CommandStatus::Accepted,
                            channel,
                            output: Some("Cancelled pending auto-restart".into()),
                            result: CommandOutcome::Completed,
                            diagnostic: None,
                        },
                    });
                    self.command_updates.send_replace(());
                    self.cancel_restart.notify_one();
                    KillResult::Accepted
                } else if data.reaped {
                    KillResult::AlreadyExited
                } else {
                    data.kill_requested = true;
                    if !matches!(data.state, State::Killing) {
                        data.state = State::Killing;
                        data.events.push(CommandEvent::Completed {
                            listener_id: input.listener_id,
                            command_id: input.command_id,
                            text: "@kill".into(),
                            result: CommandResult {
                                status: CommandStatus::Accepted,
                                channel,
                                output: None,
                                result: CommandOutcome::Sent,
                                diagnostic: None,
                            },
                        });
                        self.command_updates.send_replace(());
                        self.status.send_replace(());
                    }
                    self.kill.notify_one();
                    KillResult::Accepted
                };
                Ok(serde_json::to_value(result)?)
            }
            Request::Logs(_) => bail!("log subscription requires a dedicated channel"),
            Request::Status(_) => bail!("state subscription requires a dedicated channel"),
            Request::Commands(_) => bail!("command results require a dedicated channel"),
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
        ensure!(
            uuid::Uuid::parse_str(&input.listener_id).is_ok(),
            "invalid Listener identity"
        );
        let mut lane = self.commands.lock().await;
        let command = input.text.trim();
        let is_stop = command.strip_prefix('/').unwrap_or(command) == "stop";
        let mut restarting_cancel_result = None;
        {
            let mut data = self.data.lock().unwrap();
            if matches!(data.state, State::Restarting { .. })
                || (data.reaped && !data.state.is_exited() && data.has_restart_policy)
            {
                if !data.restart_cancelled && is_stop {
                    data.restart_cancelled = true;
                    let result = CommandResult {
                        status: CommandStatus::Accepted,
                        channel: data.channel,
                        output: Some("Cancelled pending auto-restart".into()),
                        result: CommandOutcome::Completed,
                        diagnostic: None,
                    };
                    self.cancel_restart.notify_one();
                    restarting_cancel_result = Some(result);
                } else {
                    return Ok(CommandResult {
                        status: CommandStatus::Exited,
                        channel: data.channel,
                        output: None,
                        result: CommandOutcome::Unknown,
                        diagnostic: Some(
                            "Java is restarting; send @stop or @kill to cancel".into(),
                        ),
                    });
                }
            } else if data.state.is_exited() || data.reaped {
                return Ok(CommandResult {
                    status: CommandStatus::Exited,
                    channel: data.channel,
                    output: None,
                    result: CommandOutcome::Unknown,
                    diagnostic: Some("Java has exited".into()),
                });
            }
        }
        if let Some(result) = restarting_cancel_result {
            if input.visibility == CommandVisibility::User {
                self.command_event(
                    input.listener_id,
                    input.command_id,
                    input.text,
                    result.clone(),
                );
            }
            return Ok(result);
        }
        let mut hash = blake3::Hasher::new();
        hash.update(&[u8::from(input.visibility == CommandVisibility::Internal)]);
        hash.update(input.text.as_bytes());
        let digest = hash.finalize().to_hex().to_string();
        let identity = format!("{}:{}", input.listener_id, input.command_id);
        let mut result = CommandResult {
            status: CommandStatus::Accepted,
            channel: Channel::Stdin,
            output: None,
            result: CommandOutcome::Unknown,
            diagnostic: lane.configuration_error.clone(),
        };
        let ticket = match lane.history().prepare(identity, digest, &result)? {
            history::Prepared::Existing(previous) => return Ok(previous),
            history::Prepared::Pending(ticket) => ticket,
        };
        if is_stop {
            lane.stop_attempted = true;
            self.connected(false);
            let mut data = self.data.lock().unwrap();
            if let State::Running {
                server_status,
                rcon_status,
            } = data.state
            {
                data.state = State::Stopping {
                    server_status,
                    rcon_status,
                };
                self.status.send_replace(());
            }
        }
        let attempt = match lane
            .connection
            .take()
            .filter(rcon::Connection::is_connected)
        {
            Some(connection) => Some(Ok(connection)),
            None => match &self.rcon_settings {
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
                let text = input.text.clone();
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
        if let Err(error) = lane
            .history
            .as_mut()
            .expect("active command history")
            .complete(ticket, &result)
        {
            let diagnostic = result.diagnostic.get_or_insert_with(String::new);
            diagnostic.push_str(&format!("; failed to retain command outcome: {error:#}"));
        }
        if input.visibility == CommandVisibility::User {
            self.command_event(
                input.listener_id,
                input.command_id,
                input.text,
                result.clone(),
            );
        }
        Ok(result)
    }
}

/// Verifies crash report path candidates against authoritative filesystem facts:
/// canonical containment inside `working_directory` (handling symlinked working directories
/// and Windows case/8.3 aliases while blocking symlink and `..` escapes), regular file,
/// written on or after this generation's start (floored to whole seconds for coarse
/// filesystem timestamps), and valid Minecraft crash report banner.
fn verify_crash_report(
    candidates: &[PathBuf],
    working_directory: &Path,
    started_wall: SystemTime,
) -> Option<(String, CrashReportAnalysis)> {
    let Ok(canonical_wd) = std::fs::canonicalize(working_directory) else {
        return None;
    };
    let min_mtime = floor_to_whole_seconds(started_wall);
    for candidate in candidates {
        let Ok(canonical) = std::fs::canonicalize(candidate) else {
            continue;
        };
        if !canonical.starts_with(&canonical_wd) {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&canonical) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified < min_mtime {
            continue;
        }
        let Ok(analysis) = mcpm_lib::local::mc::crash_report::analyse_file(&canonical) else {
            continue;
        };
        let display_path = logs::strip_windows_verbatim(&canonical);
        return Some((display_path.to_string_lossy().into_owned(), analysis));
    }
    None
}

fn floor_to_whole_seconds(time: SystemTime) -> SystemTime {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => SystemTime::UNIX_EPOCH + Duration::from_secs(duration.as_secs()),
        Err(_) => time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_crash_report_enforces_containment_freshness_and_valid_banner() {
        let root = tempfile::tempdir().expect("tempdir");
        let wd = root.path().join("server");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(wd.join("logs")).unwrap();
        std::fs::create_dir_all(wd.join("crash-reports")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let missing = wd.join("crash-reports/missing.txt");
        let latest_log = wd.join("logs/latest.log");
        let stale_report = wd.join("crash-reports/stale.txt");
        let fresh_report = wd.join("crash-reports/fresh.txt");
        let outside_report = outside.join("other-crash.txt");

        let report_body = "---- Minecraft Crash Report ----\n// Watchdog\n\nTime: 2026-11-28 00:00:00\nDescription: Watching Server\n";
        std::fs::write(&stale_report, report_body).unwrap();
        let stale_time = SystemTime::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&stale_report)
            .unwrap()
            .set_modified(stale_time)
            .unwrap();

        let started_wall = SystemTime::now();
        std::fs::write(&latest_log, "[00:00:00] [Server thread/INFO]: normal log\n").unwrap();
        std::fs::write(&fresh_report, report_body).unwrap();
        std::fs::write(&outside_report, report_body).unwrap();

        let mut invalid_candidates = vec![
            missing.clone(),
            latest_log.clone(),
            stale_report.clone(),
            outside_report.clone(),
            wd.join("../outside/other-crash.txt"),
        ];

        #[cfg(unix)]
        {
            // Symlink inside server directory pointing outside must be rejected.
            let escape_link = wd.join("crash-reports/escape.txt");
            std::os::unix::fs::symlink(&outside_report, &escape_link).unwrap();
            invalid_candidates.push(escape_link);

            // Symlinked working directory alias pointing to the real server directory must work.
            let wd_alias = root.path().join("server-alias");
            std::os::unix::fs::symlink(&wd, &wd_alias).unwrap();
            let aliased_candidate = wd_alias.join("crash-reports/fresh.txt");
            let (path, analysis) =
                verify_crash_report(&[aliased_candidate], &wd, started_wall)
                    .expect("symlinked working directory alias accepted");
            assert_eq!(
                PathBuf::from(path),
                logs::strip_windows_verbatim(&std::fs::canonicalize(&fresh_report).unwrap())
            );
            assert_eq!(analysis.report_type, CrashReportType::Watchdog);
        }

        // Only invalid/stale/outside candidates -> None.
        assert!(verify_crash_report(&invalid_candidates, &wd, started_wall).is_none());

        // Falls through spoofed/invalid candidates to accept the real fresh crash report.
        invalid_candidates.push(fresh_report.clone());
        let (path, analysis) = verify_crash_report(&invalid_candidates, &wd, started_wall)
            .expect("fresh valid crash report");
        assert_eq!(
            PathBuf::from(path),
            logs::strip_windows_verbatim(&std::fs::canonicalize(&fresh_report).unwrap())
        );
        assert_eq!(analysis.report_type, CrashReportType::Watchdog);
    }
}
