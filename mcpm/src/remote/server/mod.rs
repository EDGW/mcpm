//! Concurrent GUID dispatch with one writer, session errors and cooperative cancellation.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use tracing::{debug, error, info, trace};
use uuid::Uuid;

use super::command::{Cancelled, Output};
use super::protocol::{self, ErrorCode, Message, Ready, Reply, Status};
use super::registry::Registry;

mod logs;

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(crate) fn run(max_concurrency: NonZeroUsize, registry: Registry) -> Result<()> {
    serve(
        io::BufReader::new(io::stdin()),
        io::stdout().lock(),
        registry,
        max_concurrency.get(),
    )
}

fn serve(
    reader: impl BufRead + Send + 'static,
    mut writer: impl Write,
    registry: Registry,
    limit: usize,
) -> Result<()> {
    let started = Instant::now();
    info!(commands = ?registry.ids(), max_concurrency = limit, "Starting remote server");
    protocol::write(
        &mut writer,
        &Reply::new(
            Uuid::nil(),
            Status::Ready,
            Ready {
                version: protocol::VERSION,
                commands: registry.ids(),
                max_concurrency: limit,
            },
        )?,
    )?;
    debug!("Remote ready handshake sent");
    let (send, receive) = mpsc::sync_channel(64);
    let input_send = send.clone();
    thread::spawn(move || {
        let mut reader = reader;
        loop {
            let message = protocol::read(&mut reader);
            let end = !matches!(message, Ok(Some(_)));
            if input_send.send(Control::Input(message)).is_err() || end {
                break;
            }
        }
    });
    let mut tasks = Tasks::default();
    let mut seen = HashSet::new();
    let mut accepting = true;
    let mut logs: Option<logs::Logs> = None;
    while accepting || !tasks.0.is_empty() {
        if let Some(logs) = &logs {
            logs.drain(&mut writer, 64)?;
        }
        let control = match receive.recv_timeout(Duration::from_millis(10)) {
            Ok(control) => control,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(error) => return Err(error.into()),
        };
        match control {
            Control::Input(Ok(Some(Message::Logs(request)))) => {
                let guid = request.logs.guid;
                if guid.is_nil() || !seen.insert(guid) || logs.is_some() {
                    protocol::write(
                        &mut writer,
                        &Reply::error(
                            Uuid::nil(),
                            ErrorCode::Protocol,
                            "log subscription requires one unique non-nil GUID",
                        ),
                    )?;
                    bail!("invalid or duplicate log subscription");
                }
                logs = Some(logs::Logs::start(request.logs, &mut writer)?);
            }
            Control::Input(Ok(Some(Message::Request(request)))) => {
                let guid = request.guid;
                debug!(%guid, command = %request.command, active = tasks.0.len(), "Received remote command");
                if guid.is_nil() || !seen.insert(guid) {
                    debug!(%guid, "Rejecting invalid or reused invocation GUID");
                    protocol::write(
                        &mut writer,
                        &Reply::error(
                            Uuid::nil(),
                            ErrorCode::Protocol,
                            "request GUID must be non-nil and never reused",
                        ),
                    )?;
                    bail!("invalid or duplicate request GUID");
                }
                let Some(handler) = registry.handler(&request.command) else {
                    debug!(%guid, command = %request.command, "Rejecting unsupported remote command");
                    protocol::write(
                        &mut writer,
                        &Reply::error(
                            guid,
                            ErrorCode::UnsupportedCommand,
                            format!("unsupported remote command ID: {}", request.command),
                        ),
                    )?;
                    continue;
                };
                if tasks.0.len() >= limit {
                    debug!(%guid, limit, "Rejecting remote command at concurrency limit");
                    protocol::write(
                        &mut writer,
                        &Reply::error(
                            guid,
                            ErrorCode::Busy,
                            format!("remote concurrency limit ({limit}) reached"),
                        ),
                    )?;
                    continue;
                }
                let cancelled = Arc::new(AtomicBool::new(false));
                let worker_cancelled = cancelled.clone();
                let worker_send = send.clone();
                let span = tracing::info_span!("remote_command", %guid, command = %request.command);
                let worker = thread::spawn(move || {
                    let _entered = span.enter();
                    let started = Instant::now();
                    info!("Executing remote command");
                    let event_send = worker_send.clone();
                    let output = Output {
                        guid,
                        cancelled: worker_cancelled,
                        send: Arc::new(move |reply| {
                            event_send
                                .send(Control::Reply(reply))
                                .map_err(|_| anyhow!("remote session closed"))
                        }),
                    };
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        output.checkpoint()?;
                        handler(request.contents, &output)
                    }));
                    let reply = match result {
                        Ok(Ok(())) => Reply {
                            guid,
                            status: Status::Finished,
                            contents: serde_json::Value::Null,
                        },
                        Ok(Err(error)) => {
                            debug!(error = %format_args!("{error:#}"), "Remote command handler returned an error");
                            Reply::failure(
                                guid,
                                if error.is::<Cancelled>() {
                                    ErrorCode::Cancelled
                                } else {
                                    ErrorCode::HandlerFailed
                                },
                                &error,
                            )
                        }
                        Err(_) => {
                            error!("Remote command handler panicked");
                            Reply::error(guid, ErrorCode::HandlerFailed, "remote handler panicked")
                        }
                    };
                    info!(status = ?reply.status, elapsed_ms = started.elapsed().as_millis(), "Remote command execution ended");
                    let _ = worker_send.send(Control::Reply(reply));
                });
                tasks.0.insert(guid, Task { cancelled, worker });
            }
            Control::Input(Ok(Some(Message::Cancel(cancel)))) => {
                if let Some(task) = tasks.0.get(&cancel.cancel) {
                    debug!(guid = %cancel.cancel, "Cancelling active remote command");
                    task.cancelled.store(true, Ordering::Release);
                } else {
                    debug!(guid = %cancel.cancel, "Ignoring cancellation for inactive command");
                }
            }
            Control::Input(Ok(None)) => {
                debug!(
                    active = tasks.0.len(),
                    "Remote stdin closed; draining accepted commands"
                );
                accepting = false;
            }
            Control::Input(Err(error)) => {
                debug!(error = %format_args!("{error:#}"), "Remote protocol input failed");
                protocol::write(
                    &mut writer,
                    &Reply::failure(Uuid::nil(), ErrorCode::Protocol, &error),
                )?;
                return Err(error);
            }
            Control::Reply(reply) => {
                trace!(guid = %reply.guid, status = ?reply.status, "Writing remote reply");
                protocol::write(&mut writer, &reply)?;
                if reply.status.is_terminal()
                    && let Some(task) = tasks.0.remove(&reply.guid)
                {
                    let _ = task.worker.join();
                    debug!(guid = %reply.guid, active = tasks.0.len(), "Reaped completed remote command");
                }
            }
        }
    }
    info!(
        elapsed_ms = started.elapsed().as_millis(),
        "Remote server drained accepted commands"
    );
    if let Some(logs) = logs {
        logs.finish(&mut writer)?;
    }
    protocol::write(
        &mut writer,
        &Reply {
            guid: Uuid::nil(),
            status: Status::Closed,
            contents: serde_json::Value::Null,
        },
    )?;
    info!(
        elapsed_ms = started.elapsed().as_millis(),
        "Remote server closed normally"
    );
    Ok(())
}

#[derive(Debug)]
enum Control {
    Input(Result<Option<Message>>),
    Reply(Reply),
}

#[derive(Default, Debug)]
struct Tasks(HashMap<Uuid, Task>);
impl Drop for Tasks {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            debug!(
                active = self.0.len(),
                "Cancelling remaining commands during server cleanup"
            );
        }
        for task in self.0.values() {
            task.cancelled.store(true, Ordering::Release);
        }
        // Receiver teardown unblocks worker sends. A failed process session is
        // terminated by its owner even if its stdin reader is still blocked in OS I/O.
    }
}
#[derive(Debug)]
struct Task {
    cancelled: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
}

#[cfg(test)]
mod tests;
