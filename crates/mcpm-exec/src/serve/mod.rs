//! Serve loop: one task per request over a framed session, and per-call log capture.
//!
//! Requests are accepted until the caller closes stdin; the loop then cancels every call
//! still in flight, waits for its terminal frame and closes the connection. Concurrent
//! calls never block each other and the loop imposes no concurrency limit of its own.

use std::{
    collections::{HashMap, HashSet},
    io::{self, BufRead, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::{sync::mpsc, task::JoinSet};
use tracing::{Instrument, debug, info, info_span, warn};
use uuid::Uuid;

use crate::{
    error::{DispatchError, TransportError},
    protocol::{self, FailurePayload, Frame, Message},
    registry::{Call, Registry},
};

pub mod logs;

/// Frames queued for the single protocol writer.
const FRAME_CAPACITY: usize = 64;
/// Messages queued between the stdin reader and the serve loop.
const INPUT_CAPACITY: usize = 64;

/// Serves this process's host operations on stdin and stdout.
pub async fn run<P>(registry: Registry<P>, build: &str) -> Result<(), TransportError>
where
    P: Send + Sync + 'static,
{
    serve(
        io::BufReader::new(io::stdin()),
        io::stdout(),
        registry,
        build,
    )
    .await
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
async fn serve<P>(
    reader: impl BufRead + Send + 'static,
    writer: impl Write + Send + 'static,
    registry: Registry<P>,
    build: &str,
) -> Result<(), TransportError>
where
    P: Send + Sync + 'static,
{
    info!(
        operations = ?registry.ids(),
        "Starting the serve session"
    );
    let (frames, mut written) = mpsc::channel::<Frame>(FRAME_CAPACITY);
    let writer_task = tokio::task::spawn_blocking(move || -> io::Result<()> {
        let mut writer = writer;
        while let Some(frame) = written.blocking_recv() {
            protocol::write(&mut writer, &frame)?;
        }
        writer.flush()
    });
    frames
        .send(Frame::ready(build.to_owned(), registry.ids()))
        .await
        .map_err(|_| closed("the protocol writer stopped"))?;
    let (inputs, mut messages) = mpsc::channel::<Input>(INPUT_CAPACITY);
    let reader_task = tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        loop {
            let message = protocol::read::<Message>(&mut reader);
            let end = !matches!(message, Ok(Some(_)));
            if inputs.blocking_send(Input::Message(message)).is_err() || end {
                break;
            }
        }
    });

    let cancels: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>> = Arc::default();
    let mut seen = HashSet::new();
    let mut calls = JoinSet::new();
    let mut accepting = true;
    let mut failure = None;
    while accepting {
        let Some(input) = messages.recv().await else {
            break;
        };
        match input {
            Input::Message(Ok(Some(Message::Request(request)))) => {
                let guid = request.guid;
                if guid.is_nil() || !seen.insert(guid) {
                    debug!(%guid, "Rejecting invalid or reused call GUID");
                    let _ = frames
                        .send(Frame::failed(
                            Uuid::nil(),
                            FailurePayload::Dispatch {
                                error: DispatchError::Protocol {
                                    message: "request GUID must be non-nil and never reused"
                                        .to_owned(),
                                },
                            },
                        ))
                        .await;
                    failure = Some(TransportError::Io {
                        message: "the caller reused a request GUID".to_owned(),
                    });
                    accepting = false;
                    continue;
                }
                let Some(handler) = registry.handler(&request.operation) else {
                    debug!(%guid, operation = %request.operation, "Rejecting unknown operation");
                    let _ = frames
                        .send(Frame::failed(
                            guid,
                            FailurePayload::Dispatch {
                                error: DispatchError::UnknownOperation {
                                    operation: request.operation,
                                },
                            },
                        ))
                        .await;
                    continue;
                };
                debug!(%guid, operation = %request.operation, active = calls.len(), "Received call");
                let cancel = Arc::new(AtomicBool::new(false));
                cancels.lock().unwrap().insert(guid, cancel.clone());
                let subscription = logs::subscribe(guid, request.level, frames.clone());
                let cleanup = Cleanup {
                    cancels: cancels.clone(),
                    guid,
                };
                let call = Call::new(guid, cancel, frames.clone());
                let span = info_span!("operation", guid = %guid, operation = %request.operation);
                calls.spawn(
                    async move {
                        let _subscription = subscription;
                        let _cleanup = cleanup;
                        handler(request.input, call).await;
                    }
                    .instrument(span),
                );
            }
            Input::Message(Ok(Some(Message::Cancel(cancel)))) => {
                let cancels = cancels.lock().unwrap();
                match cancels.get(&cancel.cancel) {
                    Some(token) => {
                        debug!(guid = %cancel.cancel, "Cancelling the active call");
                        token.store(true, Ordering::Release);
                    }
                    None => debug!(guid = %cancel.cancel, "Ignoring a cancellation for an inactive call"),
                }
            }
            Input::Message(Ok(None)) => {
                debug!(active = calls.len(), "Remote stdin closed; cancelling accepted calls");
                accepting = false;
            }
            Input::Message(Err(error)) => {
                debug!(%error, "Remote protocol input failed");
                let _ = frames
                    .send(Frame::failed(
                        Uuid::nil(),
                        FailurePayload::Dispatch {
                            error: DispatchError::Protocol {
                                message: error.to_string(),
                            },
                        },
                    ))
                    .await;
                failure = Some(TransportError::Io {
                    message: format!("the caller sent an unreadable message: {error}"),
                });
                accepting = false;
            }
        }
    }

    let tokens: Vec<_> = cancels.lock().unwrap().values().cloned().collect();
    for token in tokens {
        token.store(true, Ordering::Release);
    }
    while calls.join_next().await.is_some() {}
    drop(reader_task);

    let written = match failure {
        Some(error) => {
            drop(frames);
            let _ = writer_task.await;
            Err(error)
        }
        None => {
            frames
                .send(Frame::closed())
                .await
                .map_err(|_| closed("the protocol writer stopped before close"))?;
            drop(frames);
            match writer_task.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(TransportError::Io {
                    message: format!("failed to write the protocol stream: {error}"),
                }),
                Err(error) => Err(TransportError::Io {
                    message: format!("the protocol writer panicked: {error}"),
                }),
            }
        }
    };
    debug!("Serve session ended");
    written
}

/// One message from the stdin reader.
#[derive(Debug)]
enum Input {
    Message(io::Result<Option<Message>>),
}

/// Removes a call from the cancellable set once its task ends.
struct Cleanup {
    cancels: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    guid: Uuid,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.cancels.lock().unwrap().remove(&self.guid);
    }
}

fn closed(message: &str) -> TransportError {
    warn!(message, "The serve session lost its protocol writer");
    TransportError::Closed {
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests;
