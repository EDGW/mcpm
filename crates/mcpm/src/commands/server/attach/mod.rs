//! Shared Local/Remote attach flow; input, logs, status and control requests remain independent.

use crate::{
    commands::{CommandOutcome as Exit, execution::Runtime},
    display::server::attach::{Display, format_exit, help_page_count},
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
const MAX_HELP_PAGES: usize = 99;
use input::Input;

/// Builds the `attach` after-help text.
fn after_help() -> clap::builder::StyledStr {
    use super::after_help::{Help, bold};

    let mut help = Help::new();
    help.heading("Modes");
    help.blank();
    help.line(&format!("  {} (terminal default)", bold("Inline TUI")));
    help.line("    The inline input stays below ordinary scrollback output.");
    help.line(&format!(
        "    {}+Up/Down changes its height.",
        crate::display::server::attach::modifier_name()
    ));
    help.blank();
    help.line(&format!("  {} ({})", bold("Console"), bold("--no-ui")));
    help.line("    Use ordinary console output without the inline interface.");
    help.blank();
    help.line(&format!(
        "  {} ({} or redirected stdout)",
        bold("Raw"),
        bold("--raw")
    ));
    help.line("    Disable TUI interaction, formatting, colors, progress, and hints.");

    help.blank();
    help.heading("Attachment");
    help.blank();
    help.line("  Attach connects to an existing Java instance and never starts Java.");
    help.line("  Command history replay defaults to one terminal screen of lines.");
    help.line(&format!(
        "  {} changes the limit; zero disables replay.",
        bold("--history-lines")
    ));
    help.line("  Re-attaching does not replay old command results.");

    help.blank();
    help.heading("Commands");
    help.blank();
    help.line("  Every attached client can send commands and receive live command results.");
    help.line("  Your own replies appear as usual; replies from another client are labeled");
    help.line("  \"Executed by another Listener\".");
    help.blank();
    help.line(&format!("  {} / EOF / Ctrl+D: detach.", bold("@detach")));
    help.line(&format!(
        "  {}: send stop without escalation.",
        bold("@stop")
    ));
    help.line(&format!("  {}: force-kill Java.", bold("@kill")));
    help.line(&format!(
        "  {}: switch the speed row between compact 100ms averages (default)",
        bold("@speed")
    ));
    help.line("          and full 100ms/1000ms lifetime peaks and averages.");
    help.line(&format!(
        "  {}: send a built-in command literally.",
        bold("@@<command>")
    ));
    help.blank();
    help.line("  Ctrl+C sends stop; another Ctrl+C while stopping force-kills.");
    help.line(&format!(
        "  In {} / {} mode, {} prints a snapshot instead of updating a row.",
        bold("Console"),
        bold("Raw"),
        bold("@speed")
    ));
    help.line("  Local runs without traffic reporting show speed as unavailable.");

    help.blank();
    help.heading("Help and hints");
    help.blank();
    help.line("  When this attachment connects over RCON, it requests help once;");
    help.line("  the result stays local to that attachment.");
    help.line("  Hints appear for nonblank input, normally match command names,");
    help.line("  wrap long syntax, and display five candidates.");
    help.line("  Recognized gamerule syntax enables rule-name completion and reports");
    help.line("  invalid type or value without blocking submission.");
    help.line("  Built-in hints remain available even if help fails.");

    help.blank();
    help.heading("Command input");
    help.blank();
    help.line("  With empty input:");
    help.line("    Up/Down recalls commands submitted in this attachment.");
    help.line("    Arrows continue through recalled entries until editing.");
    help.line("    Down past the newest entry clears input.");
    help.blank();
    help.line("  Otherwise:");
    help.line("    Up/Down browses hints with a position counter.");
    help.line("    Tab fills.");
    help.line("    Enter always sends the current input, including input with errors.");
    help.blank();
    help.line("  Leading spaces trigger a warning in the Command Input title:");
    help.line("  Minecraft may not recognize the command.");
    help.line("  Commands are never trimmed.");

    help.finish()
}

#[derive(Debug, Args)]
#[command(after_help = after_help())]
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
        if let State::Exited { reason } = &snapshot.state {
            anyhow::bail!(
                "Java has exited ({}); use launch to start a new instance",
                format_exit(reason)
            );
        }
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
    Logs(LogBatch),
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
    let initially_exited = snapshot.state.is_exited();
    let mut display = Display::new(
        no_ui,
        input_height,
        snapshot.state,
        snapshot.actual_channel,
        executor.traffic_stats(),
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
                    snapshot.logs,
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
    let mut last_restarting_attempt: Option<u32> = None;
    let mut exit_reported_for_generation = false;
    let mut pending = std::collections::VecDeque::new();
    let mut command_history = command_history::CommandHistory::default();
    let mut finished = false;
    let mut submitted = HashSet::new();
    let mut commands_finished = false;
    let mut detaching = false;
    let mut redraw = tokio::time::interval(Duration::from_millis(100));
    redraw.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut restart_redraw = tokio::time::interval(Duration::from_millis(50));
    restart_redraw.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    {
        let _keyboard = reader.pause();
        if let State::Restarting {
            previous_exit,
            rule,
            attempt,
            delay_ms,
            remaining_ms,
        } = &display.state.clone()
        {
            last_restarting_attempt = Some(*attempt);
            exit_reported_for_generation = true;
            display.exited(previous_exit)?;
            display.restarting(rule, *attempt, *delay_ms, *remaining_ms)?;
        }
        display.draw(executor.traffic_stats())?;
    }
    loop {
        if !help_requested
            && !detaching
            && display.is_tui()
            && display.state.rcon_status() == RconStatus::Connected
        {
            help_requested = true;
            let executor = executor.clone();
            let reference = reference.clone();
            let listener_id = listener_id.clone();
            help.spawn(async move {
                fetch_help_pages(&executor, &reference, &listener_id).await
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
        if detaching {
            help.abort_all();
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
        if finished
            && commands_finished
            && let Some(exit) = display.state.exit_reason().cloned()
        {
            let _keyboard = reader.pause();
            if exit_reported_for_generation {
                display.notice("restart", "Auto-restart stopped; exiting")?;
            } else {
                display.exited(&exit)?;
            }
            return Ok(Exit::Exited(exit.shell_code()));
        }
        tokio::select! {
            _ = redraw.tick() => {},
            _ = restart_redraw.tick(), if matches!(display.state, State::Restarting { .. }) => {},
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
                    Ok(pages) => display.help_result(pages.as_deref()),
                    Err(error) if error.is_cancelled() => {}
                    Err(_) => display.help_result(None),
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
                        if result.status != CommandStatus::Accepted {
                            submitted.remove(&id);
                            display.notice("command", result.diagnostic.as_deref().unwrap_or("Java has exited"))?;
                        }
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
                    let mut last_by_source = [None; 2];
                    for (index, (source, _)) in lines.iter().enumerate() {
                        last_by_source[usize::from(*source == Source::Stderr)] = Some(index);
                    }
                    for (index, (source, bytes)) in lines.into_iter().enumerate() {
                        let ends_newline = bytes.ends_with(b"\n");
                        display.logs(source, &bytes)?;
                        if !ends_newline
                            && last_by_source[usize::from(source == Source::Stderr)] != Some(index)
                        {
                            display.finish_source(source)?;
                        }
                    }
                }
                Some(Update::Logs(batch)) => {
                    if let Some(gap) = batch.gap {
                        display.finish_source(Source::Stdout)?;
                        display.finish_source(Source::Stderr)?;
                        display.notice(
                            "logs",
                            &format!("output gap {}..{} (retention exceeded)", gap.from, gap.to),
                        )?;
                    }
                    for chunk in batch.batch {
                        display.logs(chunk.source, &chunk.bytes)?;
                    }
                    if batch.finished {
                        display.finish_source(Source::Stdout)?;
                        display.finish_source(Source::Stderr)?;
                    }
                    finished = batch.finished;
                }
                Some(Update::Status(snapshot)) => {
                    let previous_restart = match &display.state {
                        State::Restarting {
                            rule,
                            attempt,
                            delay_ms,
                            ..
                        } => Some((rule.clone(), *attempt, *delay_ms)),
                        _ => None,
                    };
                    if let State::Restarting {
                        previous_exit,
                        rule,
                        attempt,
                        delay_ms,
                        remaining_ms,
                    } = &snapshot.state
                        && last_restarting_attempt != Some(*attempt)
                    {
                        last_restarting_attempt = Some(*attempt);
                        exit_reported_for_generation = true;
                        display.finish_source(Source::Stdout)?;
                        display.finish_source(Source::Stderr)?;
                        display.exited(previous_exit)?;
                        display.restarting(rule, *attempt, *delay_ms, *remaining_ms)?;
                    } else if let Some((rule, attempt, delay_ms)) = previous_restart
                        && matches!(snapshot.state, State::Running { .. })
                    {
                        help_requested = false;
                        exit_reported_for_generation = false;
                        display.restart_started(&rule, attempt, delay_ms)?;
                    }
                    display.set_state(snapshot.state);
                    display.channel = snapshot.actual_channel;
                }
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
                None => {
                    let exit = display
                        .state
                        .exit_reason()
                        .filter(|_| finished && commands_finished)
                        .cloned()
                        .context("attach readers ended without authoritative completion")?;
                    if exit_reported_for_generation {
                        display.notice("restart", "Auto-restart stopped; exiting")?;
                    } else {
                        display.exited(&exit)?;
                    }
                    return Ok(Exit::Exited(exit.shell_code()));
                }
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
    range: CursorRange,
    updates: &mpsc::Sender<Update>,
) -> Result<()> {
    let mut opened = history::open(executor, reference, cursor, &range, history_lines).await?;
    if history_lines.is_some_and(|limit| limit > 0) {
        updates
            .send(Update::History(opened.history.drain(..).collect()))
            .await?;
    }
    if let Some(batch) = opened.pending
        && (!batch.batch.is_empty() || batch.gap.is_some() || batch.finished)
    {
        updates.send(Update::Logs(batch)).await?;
    }
    while let Some(batch) = opened.channel.next().await {
        updates.send(Update::Logs(batch?)).await?;
    }
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
                exited = snapshot.state.is_exited();
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
async fn fetch_help_pages(
    executor: &Executor,
    reference: &ShimRef,
    listener_id: &str,
) -> Option<Vec<String>> {
    let first = send_help_page(executor, reference, listener_id, "help".into()).await?;
    let last_page = help_page_count(&first).unwrap_or(1).min(MAX_HELP_PAGES);
    let mut pages = Vec::with_capacity(last_page);
    pages.push(first);
    for page in 2..=last_page {
        match send_help_page(executor, reference, listener_id, format!("help {page}")).await {
            Some(output) if !output.trim().is_empty() => pages.push(output),
            _ => break,
        }
    }
    Some(pages)
}

async fn send_help_page(
    executor: &Executor,
    reference: &ShimRef,
    listener_id: &str,
    command: String,
) -> Option<String> {
    let result = send_command(
        executor,
        reference,
        listener_id,
        uuid::Uuid::new_v4().to_string(),
        command,
        CommandVisibility::Internal,
    )
    .await
    .ok()?;
    (result.status == CommandStatus::Accepted
        && result.result == shim::CommandOutcome::Completed
        && result.channel == Channel::Rcon)
        .then_some(result.output)
        .flatten()
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
                State::Running { .. } | State::Restarting { .. } => send_command(
                    &executor,
                    &reference,
                    &listener_id,
                    id.clone(),
                    "stop".into(),
                    CommandVisibility::User,
                )
                .await
                .map(Action::Command),
                State::Stopping { .. } | State::Killing => {
                    send_kill(&executor, &reference, &listener_id, id.clone())
                        .await
                        .map(Action::Kill)
                }
                State::Exited { .. } => Ok(Action::Kill(KillResult::AlreadyExited)),
            }
        }
        .await;
        (id, result)
    });
}
