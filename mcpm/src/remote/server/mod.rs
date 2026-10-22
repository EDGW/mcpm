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

use anyhow::{Result, anyhow, bail};
use uuid::Uuid;

use super::command::{Cancelled, Output};
use super::protocol::{self, ErrorCode, Message, Ready, Reply, Status};
use super::registry::Registry;

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
    while accepting || !tasks.0.is_empty() {
        match receive.recv()? {
            Control::Input(Ok(Some(Message::Request(request)))) => {
                let guid = request.guid;
                if guid.is_nil() || !seen.insert(guid) {
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
                let worker = thread::spawn(move || {
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
                        Ok(Err(error)) => Reply::error(
                            guid,
                            if error.is::<Cancelled>() {
                                ErrorCode::Cancelled
                            } else {
                                ErrorCode::HandlerFailed
                            },
                            format!("{error:#}"),
                        ),
                        Err(_) => {
                            Reply::error(guid, ErrorCode::HandlerFailed, "remote handler panicked")
                        }
                    };
                    let _ = worker_send.send(Control::Reply(reply));
                });
                tasks.0.insert(guid, Task { cancelled, worker });
            }
            Control::Input(Ok(Some(Message::Cancel(cancel)))) => {
                if let Some(task) = tasks.0.get(&cancel.cancel) {
                    task.cancelled.store(true, Ordering::Release);
                }
            }
            Control::Input(Ok(None)) => accepting = false,
            Control::Input(Err(error)) => {
                protocol::write(
                    &mut writer,
                    &Reply::error(Uuid::nil(), ErrorCode::Protocol, format!("{error:#}")),
                )?;
                return Err(error);
            }
            Control::Reply(reply) => {
                protocol::write(&mut writer, &reply)?;
                if reply.status.is_terminal()
                    && let Some(task) = tasks.0.remove(&reply.guid)
                {
                    let _ = task.worker.join();
                }
            }
        }
    }
    protocol::write(
        &mut writer,
        &Reply {
            guid: Uuid::nil(),
            status: Status::Closed,
            contents: serde_json::Value::Null,
        },
    )
}

enum Control {
    Input(Result<Option<Message>>),
    Reply(Reply),
}

#[derive(Default, Debug)]
struct Tasks(HashMap<Uuid, Task>);
impl Drop for Tasks {
    fn drop(&mut self) {
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
