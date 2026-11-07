//! Shared Local/Remote attach flow; input, logs, status and control requests remain independent.

use crate::{
    commands::{CommandOutcome as Exit, execution::Runtime},
    display::server::attach::Display,
    ssh::SshArgs,
};
use anyhow::{Context, Result, ensure};
use clap::Args;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use mcpm_backend::operations::server::shim::{self, *};
use mcpm_exec::service::Executor;
use std::{collections::HashSet, path::PathBuf, time::Duration};
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

mod builtin;
mod command_history;
mod history;
mod input;
pub(super) const DEFAULT_INPUT_HEIGHT: u16 = 4;
use input::Input;

#[derive(Debug, Args)]
#[command(
    after_help = super::after_help::attach()
)]
pub(super) struct AttachArgs {
    /// Registered server directory on the selected Host.
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
    /// Number of retained history lines to replay; defaults to terminal height (24 when unavailable).
    #[arg(long, value_name = "LINES")]
    history_lines: Option<usize>,
    /// Initial inline input height, adjustable while attached.
    #[arg(long, default_value_t = DEFAULT_INPUT_HEIGHT, value_parser = clap::value_parser!(u16).range(3..), help = format!("Initial inline input height; {}+Up/Down adjusts it while attached", crate::display::server::attach::modifier_name()))]
    input_height: u16,
    /// Disable the inline interface and use Console/Raw output.
    #[arg(long)]
    no_ui: bool,
}
pub(super) fn run(args: AttachArgs, ssh: SshArgs) -> Result<Exit> {
    let runtime = Runtime::new()?;
    runtime.block_on(async move {
        let (executor, server, _) = ssh.create_executor(Some(args.server)).await?;
        let snapshot = executor
            .call::<Inspect>(InspectInput {
                server: Some(server.context("attach requires a server path")?),
                run_id: None,
            })
            .await?;
        ensure!(
            snapshot.state != State::Exited,
            "Java has exited (code {:?}); use launch to start a new instance",
            snapshot.exit_code
        );
        flow(
            executor,
            snapshot.reference,
            None,
            Some(args.history_lines.unwrap_or_else(|| {
                crossterm::terminal::size().map_or(24, |(_, height)| height as usize)
            })),
            args.no_ui,
            args.input_height,
        )
        .await
    })
}

enum Update {
    Logs(Source, LogBatch),
    History(Vec<(Source, Vec<u8>)>),
    Status(Snapshot),
    Command(CommandEvent),
    Failed(anyhow::Error),
}
enum Action {
    Command(CommandResult),
    Kill(KillResult),
}
struct Tasks(Vec<JoinHandle<()>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

pub(super) async fn flow(
    executor: Executor,
    reference: ShimRef,
    cursor: Option<u64>,
    history_lines: Option<usize>,
    no_ui: bool,
    input_height: u16,
) -> Result<Exit> {
    let status_input = StatusInput {
        reference: reference.clone(),
        interval_ms: StatusInput::DEFAULT_INTERVAL_MS,
    };
    let status_timeout = status_input.timeout();
    let mut status_channel = tokio::time::timeout(
        status_timeout,
        executor.call::<SubscribeStatus>(status_input),
    )
    .await
    .context("state Channel handshake timed out; Java's exit is not confirmed")??;
    let snapshot = tokio::time::timeout(status_timeout, status_channel.next())
        .await
        .context("initial state Channel snapshot timed out; Java's exit is not confirmed")?
        .context("state Channel closed without an initial snapshot")??;
    let initially_exited = snapshot.state == State::Exited;
    let mut display = Display::new(
        no_ui,
        input_height,
        snapshot.state,
        snapshot.actual_channel,
        snapshot.server_status,
    )?;
    display.builtin_hints(
        builtin::DEFINITIONS
            .iter()
            .map(|entry| (entry.name, entry.description)),
    );
    let mut reader = input::Reader::new(display.is_tui())?;
    let mut diagnostics_open = display.is_tui();
    #[cfg(unix)]
    let mut interrupts = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    #[cfg(windows)]
    let mut interrupts = tokio::signal::windows::ctrl_c()?;
    let listener_id = uuid::Uuid::new_v4().to_string();
    let mut result_channel = tokio::time::timeout(
        status_timeout,
        executor.call::<SubscribeCommands>(CommandsInput {
            reference: reference.clone(),
        }),
    )
    .await
    .context("command result subscription timed out")??;
    ensure!(
        matches!(
            tokio::time::timeout(status_timeout, result_channel.next())
                .await
                .context("command result subscription handshake timed out")?
                .context("command result Channel closed before Ready")??,
            CommandEvent::Ready
        ),
        "command result Channel did not establish a live subscription"
    );
    let (updates, mut events) = mpsc::channel(8);
    let _tasks = Tasks(vec![
        tokio::spawn({
            let executor = executor.clone();
            let reference = reference.clone();
            let updates = updates.clone();
            async move {
                let result = follow_logs(
                    &executor,
                    &reference,
                    cursor,
                    history_lines,
                    [snapshot.stdout, snapshot.stderr],
                    &updates,
                )
                .await;
                if let Err(error) = result {
                    let _ = updates.send(Update::Failed(error)).await;
                }
            }
        }),
        tokio::spawn({
            let updates = updates.clone();
            async move {
                if let Err(error) =
                    follow_status(status_channel, status_timeout, initially_exited, &updates).await
                {
                    let _ = updates.send(Update::Failed(error)).await;
                }
            }
        }),
        tokio::spawn({
            let updates = updates.clone();
            async move {
                let result = async {
                    while let Some(event) = result_channel.next().await {
                        let event = event?;
                        let done = matches!(event, CommandEvent::Done);
                        updates.send(Update::Command(event)).await?;
                        if done {
                            return Ok::<_, anyhow::Error>(());
                        }
                    }
                    anyhow::bail!("command result Channel closed before Done")
                }
                .await;
                if let Err(error) = result {
                    let _ = updates.send(Update::Failed(error)).await;
                }
            }
        }),
    ]);
    drop(updates);
    let mut actions = JoinSet::new();
    let mut commands = JoinSet::new();
    let mut help = JoinSet::new();
    let mut help_requested = false;
    let mut pending = std::collections::VecDeque::new();
    let mut command_history = command_history::CommandHistory::default();
    let mut finished = [false; 2];
    let mut exit_code = snapshot.exit_code;
    let mut submitted = HashSet::new();
    let mut commands_finished = false;
    let mut detaching = false;
    let mut redraw = tokio::time::interval(Duration::from_millis(100));
    {
        let _keyboard = reader.pause();
        display.draw(executor.traffic_stats())?;
    }
    loop {
        if !help_requested && !detaching && display.server_status == ServerStatus::RconConnected {
            help_requested = true;
            let executor = executor.clone();
            let reference = reference.clone();
            let listener_id = listener_id.clone();
            help.spawn(async move {
                send_command(
                    &executor,
                    &reference,
                    &listener_id,
                    uuid::Uuid::new_v4().to_string(),
                    "help".into(),
                    CommandVisibility::Internal,
                )
                .await
            });
        }
        if commands.is_empty()
            && let Some(text) = pending.pop_front()
        {
            let executor = executor.clone();
            let reference = reference.clone();
            let listener_id = listener_id.clone();
            let id = uuid::Uuid::new_v4().to_string();
            submitted.insert(id.clone());
            commands.spawn(async move {
                let result = send_command(
                    &executor,
                    &reference,
                    &listener_id,
                    id.clone(),
                    text,
                    CommandVisibility::User,
                )
                .await;
                (id, result)
            });
        }
        if detaching
            && actions.is_empty()
            && commands.is_empty()
            && pending.is_empty()
            && help.is_empty()
            && submitted.is_empty()
        {
            return Ok(Exit::Completed);
        }
        if finished.iter().all(|finished| *finished)
            && commands_finished
            && let Some(code) = exit_code
        {
            return Ok(Exit::Exited(code));
        }
        tokio::select! {
            _ = redraw.tick() => {},
            diagnostic = display.diagnostics.recv(), if diagnostics_open => {
                let _keyboard = reader.pause();
                match diagnostic {
                    Some((bytes, dropped)) => display.diagnostic(&bytes, dropped)?,
                    None => diagnostics_open = false,
                }
            }
            signal = interrupts.recv() => {
                ensure!(signal.is_some(), "interrupt notification stream ended");
                spawn_interrupt(&mut actions, &mut submitted, executor.clone(), reference.clone(), listener_id.clone());
            }
            result = help.join_next(), if !help.is_empty() => {
                match result.context("help task disappeared")? {
                    Ok(Ok(result)) if result.status == CommandStatus::Accepted
                        && result.result == shim::CommandOutcome::Completed && result.channel == Channel::Rcon => {
                        display.help_result(result.output.as_deref());
                    }
                    _ => display.help_result(None),
                }
            }
            command = commands.join_next(), if !commands.is_empty() => {
                let _keyboard = reader.pause();
                match command.context("console command disappeared")? {
                    Ok((id, Ok(result))) => {
                        if result.status != CommandStatus::Accepted {
                            submitted.remove(&id);
                            display.notice("command", result.diagnostic.as_deref().unwrap_or("Java has exited"))?;
                        }
                    }
                    Ok((id, Err(error))) => { submitted.remove(&id); display.notice("command", &format!("{error:#}"))?; }
                    Err(error) => return Err(error).context("console command task failed; Java was not stopped automatically"),
                }
            }
            action = actions.join_next(), if !actions.is_empty() => {
                let _keyboard = reader.pause();
                match action.context("control action disappeared")? {
                    Ok((id, Ok(Action::Command(result)))) => {
                        if result.status != CommandStatus::Accepted { submitted.remove(&id); display.notice("command", "Java has exited")?; }
                    }
                    Ok((id, Ok(Action::Kill(result)))) => {
                        if result == KillResult::AlreadyExited { submitted.remove(&id); display.notice("kill", "Java has exited")?; }
                    }
                    Ok((id, Err(error))) => { submitted.remove(&id); display.notice("control", &format!("{error:#}"))?; }
                    Err(error) => return Err(error).context("attach control task failed; Java was not stopped automatically"),
                }
            }
            event = events.recv() => {
                let _keyboard = reader.pause();
                match event {
                Some(Update::History(lines)) => {
                    if !lines.is_empty() { display.history_divider()?; }
                    for (source, bytes) in lines { display.logs(source, &bytes)?; }
                }
                Some(Update::Logs(source, batch)) => {
                    if let Some(gap) = batch.gap { display.notice("logs", &format!("{source:?}: output gap {}..{} (retention exceeded)", gap.from, gap.to))?; }
                    for chunk in batch.batch { display.logs(source, &chunk.bytes)?; }
                    if batch.finished { display.finish_source(source)?; }
                    finished[usize::from(source == Source::Stderr)] = batch.finished;
                }
                Some(Update::Status(snapshot)) => { display.state = snapshot.state; display.channel = snapshot.actual_channel; display.server_status = snapshot.server_status; exit_code = snapshot.exit_code; }
                Some(Update::Command(CommandEvent::Completed { listener_id: origin, command_id, text, result })) => {
                    let other = origin != listener_id;
                    if !other { submitted.remove(&command_id); }
                    display.command(&text, &result, other, &origin)?;
                }
                Some(Update::Command(CommandEvent::Gap { missed })) => {
                    submitted.clear();
                    display.notice("commands", &format!("{missed} command results omitted; Listener fell behind"))?;
                }
                Some(Update::Command(CommandEvent::Done)) => { commands_finished = true; submitted.clear(); }
                Some(Update::Command(CommandEvent::Ready)) => return Err(anyhow::anyhow!("unexpected repeated result Channel handshake")),
                Some(Update::Failed(error)) => return Err(error).context("attach lost the shim; Java was not stopped automatically"),
                None => { ensure!(finished.iter().all(|finished| *finished) && exit_code.is_some() && commands_finished, "attach readers ended without authoritative completion"); return Ok(Exit::Exited(exit_code.unwrap())); }
                }
            },
            input = reader.events.recv(), if !detaching => {
                let mut line = None;
                match input {
                    None | Some(Input::Eof) => detaching = true,
                    Some(Input::Error(error)) => return Err(error).context("attach input failed; Java was not stopped automatically"),
                    Some(Input::Line(text)) => line = Some(text),
                    Some(Input::Key(key)) if key.kind != KeyEventKind::Release => {
                        if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Up | KeyCode::Down) {
                            let _keyboard = reader.pause();
                            display.resize_input(key.code == KeyCode::Up)?;
                        } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                            match key.code {
                                KeyCode::Char('d') => detaching = true,
                                KeyCode::Char('c') => spawn_interrupt(&mut actions, &mut submitted, executor.clone(), reference.clone(), listener_id.clone()),
                                _ => {},
                            }
                        } else {
                            match key.code {
                                KeyCode::Enter => { display.reset_suggestion(); display.cursor = 0; line = Some(std::mem::take(&mut display.draft)); }
                                KeyCode::Tab => { command_history.reset(); display.accept_suggestion(); }
                                KeyCode::Up | KeyCode::Down => {
                                    let down = key.code == KeyCode::Down;
                                    if command_history.navigate(&mut display.draft, down) {
                                        display.cursor = display.draft.len();
                                        display.reset_suggestion();
                                    } else {
                                        display.move_suggestion(down);
                                    }
                                }
                                KeyCode::Char(ch) if display.draft.len() + ch.len_utf8() <= 16 * 1024 => { command_history.reset(); display.reset_suggestion(); display.draft.insert(display.cursor, ch); display.cursor += ch.len_utf8(); }
                                KeyCode::Backspace if display.cursor > 0 => { command_history.reset(); display.reset_suggestion(); let previous = previous(&display.draft, display.cursor); display.draft.drain(previous..display.cursor); display.cursor = previous; }
                                KeyCode::Delete if display.cursor < display.draft.len() => { command_history.reset(); display.reset_suggestion(); let next = next(&display.draft, display.cursor); display.draft.drain(display.cursor..next); }
                                KeyCode::Left => display.cursor = previous(&display.draft, display.cursor),
                                KeyCode::Right => display.cursor = next(&display.draft, display.cursor),
                                KeyCode::Home => display.cursor = 0,
                                KeyCode::End => display.cursor = display.draft.len(),
                                _ => {},
                            }
                        }
                    }
                    Some(Input::Key(_)) => {},
                }
                if let Some(mut text) = line {
                    if display.is_tui() {
                        command_history.record(&text);
                    }
                    let builtin = builtin::parse(&text);
                    if matches!(builtin, Some(builtin::Builtin::Detach)) { detaching = true; }
                    else if matches!(builtin, Some(builtin::Builtin::Speed)) {
                        let _keyboard = reader.pause();
                        display.toggle_speed(executor.traffic_stats())?;
                    }
                    else if matches!(builtin, Some(builtin::Builtin::Kill)) {
                        ensure!(actions.len() < 32, "too many pending control actions; Java was not stopped automatically");
                        let executor = executor.clone();
                        let reference = reference.clone();
                        let listener_id = listener_id.clone();
                        let id = uuid::Uuid::new_v4().to_string();
                        submitted.insert(id.clone());
                        actions.spawn(async move {
                            let result = send_kill(&executor, &reference, &listener_id, id.clone()).await.map(Action::Kill);
                            (id, result)
                        });
                    } else if !text.is_empty() {
                        if matches!(builtin, Some(builtin::Builtin::Stop)) {
                            text = "stop".into();
                        } else if text.strip_prefix('@').and_then(builtin::parse).is_some() {
                            text.remove(0);
                        }
                        ensure!(pending.len() + commands.len() < 32, "too many pending console commands; Java was not stopped automatically");
                        pending.push_back(text);
                    }
                }
            }
        }
        {
            let _keyboard = reader.pause();
            display.draw(executor.traffic_stats())?;
        }
    }
}
fn previous(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}
fn next(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .chars()
        .next()
        .map_or(cursor, |ch| cursor + ch.len_utf8())
}

async fn follow_logs(
    executor: &Executor,
    reference: &ShimRef,
    cursor: Option<u64>,
    history_lines: Option<usize>,
    ranges: [CursorRange; 2],
    updates: &mpsc::Sender<Update>,
) -> Result<()> {
    let (mut stdout, mut stderr) = tokio::try_join!(
        history::open(
            executor,
            reference,
            Source::Stdout,
            cursor,
            &ranges[0],
            history_lines
        ),
        history::open(
            executor,
            reference,
            Source::Stderr,
            cursor,
            &ranges[1],
            history_lines
        ),
    )?;
    if let Some(limit) = history_lines.filter(|limit| *limit > 0) {
        let mut lines: std::collections::VecDeque<_> = stdout
            .history
            .drain(..)
            .map(|bytes| (Source::Stdout, bytes))
            .chain(
                stderr
                    .history
                    .drain(..)
                    .map(|bytes| (Source::Stderr, bytes)),
            )
            .collect();
        while lines.len() > limit {
            lines.pop_front();
        }
        updates.send(Update::History(lines.into())).await?;
    }
    async fn source(
        mut opened: history::Opened,
        kind: Source,
        updates: &mpsc::Sender<Update>,
    ) -> Result<()> {
        if let Some(batch) = opened.pending {
            if !batch.batch.is_empty() || batch.gap.is_some() || batch.finished {
                updates.send(Update::Logs(kind, batch)).await?;
            }
        }
        while let Some(batch) = opened.channel.next().await {
            updates.send(Update::Logs(kind, batch?)).await?;
        }
        Ok(())
    }
    tokio::try_join!(
        source(stdout, Source::Stdout, updates),
        source(stderr, Source::Stderr, updates)
    )?;
    Ok(())
}

async fn follow_status(
    mut channel: mcpm_exec::operation::StreamingOutput<Snapshot>,
    timeout: Duration,
    mut exited: bool,
    updates: &mpsc::Sender<Update>,
) -> Result<()> {
    loop {
        match tokio::time::timeout(timeout, channel.next())
            .await
            .context("state Channel heartbeat timed out; Java's exit is not confirmed")?
        {
            Some(snapshot) => {
                let snapshot = snapshot?;
                exited = snapshot.state == State::Exited;
                updates.send(Update::Status(snapshot)).await?;
            }
            None => {
                ensure!(
                    exited,
                    "state Channel disconnected without a final Java exit snapshot"
                );
                return Ok(());
            }
        }
    }
}
async fn send_command(
    executor: &Executor,
    reference: &ShimRef,
    listener_id: &str,
    command_id: String,
    text: String,
    visibility: CommandVisibility,
) -> Result<CommandResult> {
    executor
        .call::<shim::Command>(CommandInput {
            reference: reference.clone(),
            listener_id: listener_id.into(),
            command_id,
            text,
            visibility,
        })
        .await
        .context("command outcome unknown on communication failure; do not replay")
}

async fn send_kill(
    executor: &Executor,
    reference: &ShimRef,
    listener_id: &str,
    command_id: String,
) -> Result<KillResult> {
    executor
        .call::<Kill>(ControlInput {
            reference: reference.clone(),
            listener_id: listener_id.into(),
            command_id,
        })
        .await
}

fn spawn_interrupt(
    actions: &mut JoinSet<(String, Result<Action>)>,
    submitted: &mut HashSet<String>,
    executor: Executor,
    reference: ShimRef,
    listener_id: String,
) {
    if actions.len() >= 2 {
        return;
    }
    let id = uuid::Uuid::new_v4().to_string();
    submitted.insert(id.clone());
    actions.spawn(async move {
        let result = async {
            let info = executor
                .call::<Inspect>(InspectInput {
                    server: Some(reference.server.clone()),
                    run_id: Some(reference.run_id.clone()),
                })
                .await?;
            match info.state {
                State::Running => send_command(
                    &executor,
                    &reference,
                    &listener_id,
                    id.clone(),
                    "stop".into(),
                    CommandVisibility::User,
                )
                .await
                .map(Action::Command),
                State::Stopping | State::Killing => {
                    send_kill(&executor, &reference, &listener_id, id.clone())
                        .await
                        .map(Action::Kill)
                }
                State::Exited => Ok(Action::Kill(KillResult::AlreadyExited)),
            }
        }
        .await;
        (id, result)
    });
}
