//! Shared Local/Remote attach flow; input, logs, status and control requests remain independent.

use crate::{
    commands::{CommandOutcome as Exit, execution::Runtime},
    display::server::attach::Display,
    ssh::SshArgs,
};
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use mcpm_backend::operations::server::shim::{self, *};
use mcpm_exec::service::Executor;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, mpsc},
    task::{JoinHandle, JoinSet},
};

mod builtin;
mod history;
mod input;
pub(super) const DEFAULT_INPUT_HEIGHT: u16 = 4;
use input::Input;

#[derive(Debug, Args)]
#[command(
    after_help = format!("Modes: Inline TUI (terminal default), Console (--no-ui), Raw (--raw or redirected stdout). The inline input stays below ordinary scrollback output; {}+Up/Down changes its height. Attach never starts Java. History defaults to one terminal screen of lines; --history-lines changes it and zero skips replay. RCON replies have a Primary-colored frame with the command name. @detach or EOF/Ctrl+D detaches; @stop sends stop without escalation; @kill force-kills Java. Prefix a builtin with another @ to send it literally. Ctrl+C sends stop; another while Stopping force-kills. Each attach requests help once after RCON Connected, storing it only in that frontend session. Inline hints appear only while input is nonblank, match command names only, wrap long syntax, and display five candidates. Up/Down browses all matches with a position counter. Tab fills; Enter always sends the current input. Builtin hints remain available if help fails.", crate::display::server::attach::modifier_name())
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
    Failed(anyhow::Error),
}
enum Action {
    Command((String, CommandResult)),
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
type Token = Arc<Mutex<Option<String>>>;

pub(super) async fn flow(
    executor: Executor,
    reference: ShimRef,
    initial_token: Option<String>,
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
    let token = Arc::new(Mutex::new(initial_token));
    let requester = uuid::Uuid::new_v4().to_string();
    if token.lock().await.is_none() {
        match refresh_token(&executor, &reference, &token, &requester, None).await {
            Ok(_) => {}
            Err(error) => display.notice(
                "control",
                &format!(
                    "{error:#}; logs remain available, retry a command after the holder expires"
                ),
            )?,
        }
    }
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
    ]);
    drop(updates);
    let mut actions = JoinSet::new();
    let mut commands = JoinSet::new();
    let mut help = JoinSet::new();
    let mut help_requested = false;
    let mut pending = std::collections::VecDeque::new();
    let mut finished = [false; 2];
    let mut exit_code = snapshot.exit_code;
    let mut detaching = false;
    let mut redraw = tokio::time::interval(Duration::from_millis(100));
    {
        let _keyboard = reader.pause();
        display.draw()?;
    }
    loop {
        if !help_requested && !detaching && display.server_status == ServerStatus::RconConnected {
            help_requested = true;
            let executor = executor.clone();
            let reference = reference.clone();
            let token = token.clone();
            let requester = requester.clone();
            help.spawn(async move {
                controlled_command(&executor, &reference, &token, &requester, "help".into()).await
            });
        }
        if commands.is_empty()
            && let Some(text) = pending.pop_front()
        {
            let executor = executor.clone();
            let reference = reference.clone();
            let token = token.clone();
            let requester = requester.clone();
            commands.spawn(async move {
                controlled_command(&executor, &reference, &token, &requester, text).await
            });
        }
        if detaching
            && actions.is_empty()
            && commands.is_empty()
            && pending.is_empty()
            && help.is_empty()
        {
            return Ok(Exit::Completed);
        }
        if finished.iter().all(|finished| *finished)
            && let Some(code) = exit_code
        {
            return Ok(Exit::Exited(code));
        }
        tokio::select! {
            _ = redraw.tick() => {},
            diagnostic = display.diagnostics.recv(), if diagnostics_open => {
                match diagnostic {
                    Some((bytes, dropped)) => display.diagnostic(&bytes, dropped)?,
                    None => diagnostics_open = false,
                }
            }
            signal = interrupts.recv() => {
                ensure!(signal.is_some(), "interrupt notification stream ended");
                spawn_interrupt(&mut actions, executor.clone(), reference.clone(), token.clone(), requester.clone());
            }
            result = help.join_next(), if !help.is_empty() => {
                match result.context("help task disappeared")? {
                    Ok(Ok((_, result))) if result.status == CommandStatus::Accepted
                        && result.result == shim::CommandOutcome::Completed && result.channel == Channel::Rcon => {
                        display.help_result(result.output.as_deref());
                    }
                    _ => display.help_result(None),
                }
            }
            command = commands.join_next(), if !commands.is_empty() => {
                match command.context("console command disappeared")? {
                    Ok(Ok((text, result))) => display.command(&text, &result)?,
                    Ok(Err(error)) => display.notice("command", &format!("{error:#}"))?,
                    Err(error) => return Err(error).context("console command task failed; Java was not stopped automatically"),
                }
            }
            action = actions.join_next(), if !actions.is_empty() => {
                match action.context("control action disappeared")? {
                    Ok(Ok(Action::Command((text, result)))) => display.command(&text, &result)?,
                    Ok(Ok(Action::Kill(result))) => display.notice("kill", &format!("{result:?}"))?,
                    Ok(Err(error)) => display.notice("control", &format!("{error:#}"))?,
                    Err(error) => return Err(error).context("attach control task failed; Java was not stopped automatically"),
                }
            }
            event = events.recv() => match event {
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
                Some(Update::Failed(error)) => return Err(error).context("attach lost the shim; Java was not stopped automatically"),
                None => { ensure!(finished.iter().all(|finished| *finished) && exit_code.is_some(), "attach readers ended without an authoritative Java exit"); return Ok(Exit::Exited(exit_code.unwrap())); }
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
                                KeyCode::Char('c') => spawn_interrupt(&mut actions, executor.clone(), reference.clone(), token.clone(), requester.clone()),
                                _ => {},
                            }
                        } else {
                            match key.code {
                                KeyCode::Enter => { display.reset_suggestion(); display.cursor = 0; line = Some(std::mem::take(&mut display.draft)); }
                                KeyCode::Tab => display.accept_suggestion(),
                                KeyCode::Up => display.move_suggestion(false),
                                KeyCode::Down => display.move_suggestion(true),
                                KeyCode::Char(ch) if display.draft.len() + ch.len_utf8() <= 16 * 1024 => { display.reset_suggestion(); display.draft.insert(display.cursor, ch); display.cursor += ch.len_utf8(); }
                                KeyCode::Backspace if display.cursor > 0 => { display.reset_suggestion(); let previous = previous(&display.draft, display.cursor); display.draft.drain(previous..display.cursor); display.cursor = previous; }
                                KeyCode::Delete if display.cursor < display.draft.len() => { display.reset_suggestion(); let next = next(&display.draft, display.cursor); display.draft.drain(display.cursor..next); }
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
                    let builtin = builtin::parse(&text);
                    if matches!(builtin, Some(builtin::Builtin::Detach)) { detaching = true; }
                    else if matches!(builtin, Some(builtin::Builtin::Kill)) {
                        ensure!(actions.len() < 32, "too many pending control actions; Java was not stopped automatically");
                        let executor = executor.clone();
                        let reference = reference.clone();
                        let token = token.clone();
                        let requester = requester.clone();
                        actions.spawn(async move {
                            controlled_kill(&executor, &reference, &token, &requester).await.map(Action::Kill)
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
            display.draw()?;
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
async fn refresh_token(
    executor: &Executor,
    reference: &ShimRef,
    token: &Token,
    requester: &str,
    failed: Option<&str>,
) -> Result<String> {
    let mut current = token.lock().await;
    if let Some(value) = current.as_ref()
        && failed != Some(value.as_str())
    {
        return Ok(value.clone());
    }
    match executor
        .call::<Acquire>(AcquireInput {
            reference: reference.clone(),
            requester: requester.into(),
        })
        .await?
    {
        AcquireResult::Acquired { token } => {
            *current = Some(token.clone());
            Ok(token)
        }
        AcquireResult::Busy { current_holder } => {
            bail!("control is busy ({current_holder}); no command was sent")
        }
    }
}
async fn controlled_command(
    executor: &Executor,
    reference: &ShimRef,
    token: &Token,
    requester: &str,
    text: String,
) -> Result<(String, CommandResult)> {
    let mut value = refresh_token(executor, reference, token, requester, None).await?;
    let id = uuid::Uuid::new_v4().to_string();
    for _ in 0..2 {
        let result = executor
            .call::<shim::Command>(CommandInput {
                reference: reference.clone(),
                control_token: value.clone(),
                command_id: id.clone(),
                text: text.clone(),
            })
            .await
            .context("command outcome unknown on communication failure; do not replay")?;
        if result.status != CommandStatus::InvalidToken {
            return Ok((text, result));
        }
        value = refresh_token(executor, reference, token, requester, Some(&value)).await?;
    }
    bail!("control token changed; no command was accepted")
}
async fn controlled_kill(
    executor: &Executor,
    reference: &ShimRef,
    token: &Token,
    requester: &str,
) -> Result<KillResult> {
    let mut value = refresh_token(executor, reference, token, requester, None).await?;
    let mut result = executor
        .call::<Kill>(ControlInput {
            reference: reference.clone(),
            control_token: value.clone(),
        })
        .await?;
    if result == KillResult::InvalidToken {
        value = refresh_token(executor, reference, token, requester, Some(&value)).await?;
        result = executor
            .call::<Kill>(ControlInput {
                reference: reference.clone(),
                control_token: value,
            })
            .await?;
    }
    Ok(result)
}
fn spawn_interrupt(
    actions: &mut JoinSet<Result<Action>>,
    executor: Executor,
    reference: ShimRef,
    token: Token,
    requester: String,
) {
    if actions.len() >= 2 {
        return;
    }
    actions.spawn(async move {
        let info = executor
            .call::<Inspect>(InspectInput {
                server: Some(reference.server.clone()),
                run_id: Some(reference.run_id.clone()),
            })
            .await?;
        match info.state {
            State::Running => {
                controlled_command(&executor, &reference, &token, &requester, "stop".into())
                    .await
                    .map(Action::Command)
            }
            State::Stopping | State::Killing => {
                controlled_kill(&executor, &reference, &token, &requester)
                    .await
                    .map(Action::Kill)
            }
            State::Exited => Ok(Action::Kill(KillResult::AlreadyExited)),
        }
    });
}
