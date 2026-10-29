//! Host operation lookup for the serve loop: JSON in, JSON events and one JSON result out.
//!
//! The registry erases an operation into one JSON handler so serve dispatches without
//! knowing any operation type. Encoding happens where the operation declares its codec,
//! and the decoded operation error stays the operation's own value.

use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use serde_json::Value;
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    error::{Cancelled, DispatchError, Failure},
    operation::{Operation, Provide, Wire},
    protocol::{FailurePayload, Frame},
    transport::inprocess,
};

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type Handler = Arc<dyn Fn(Value, Call) -> BoxFuture + Send + Sync>;

/// Host operations by identifier, each ready to run against the registry's provider.
pub struct Registry<P> {
    provider: P,
    handlers: BTreeMap<&'static str, Handler>,
}

impl<P> Registry<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            handlers: BTreeMap::new(),
        }
    }

    /// Identifiers of every registered operation, in stable order.
    pub fn ids(&self) -> Vec<String> {
        self.handlers.keys().map(|id| (*id).to_owned()).collect()
    }

    /// Registers one host operation; its codec must be declared by its registration macro.
    pub fn register<O>(&mut self) -> Result<(), RegistryError>
    where
        O: Operation,
        P: Provide<O::Env> + Clone + Send + Sync + 'static,
    {
        if O::ID.is_empty() {
            return Err(RegistryError::EmptyId);
        }
        if self.handlers.contains_key(O::ID) {
            return Err(RegistryError::DuplicateId { id: O::ID });
        }
        let wire = O::WIRE.ok_or(RegistryError::NotRemotable { id: O::ID })?;
        let provider = self.provider.clone();
        let handler: Handler = Arc::new(move |input: Value, call: Call| {
            let provider = provider.clone();
            Box::pin(async move {
                let input = match (wire.decode_input)(input) {
                    Ok(input) => input,
                    Err(error) => {
                        let _ = call
                            .finish(Frame::failed(
                                call.guid(),
                                FailurePayload::Dispatch { error },
                            ))
                            .await;
                        return;
                    }
                };
                let mut started =
                    inprocess::start::<O>(provider.provide(), input, call.cancel_token());
                while let Some(event) = started.events.recv().await {
                    let frame = match (wire.encode_event)(event) {
                        Ok(event) => Frame::data(call.guid(), event),
                        Err(error) => {
                            started.task.abort();
                            let _ = call
                                .finish(Frame::failed(
                                    call.guid(),
                                    FailurePayload::Dispatch { error },
                                ))
                                .await;
                            return;
                        }
                    };
                    if call.send(frame).await.is_err() {
                        // The call was cancelled or the session is gone; still report the end
                        // while the session can accept frames.
                        started.task.abort();
                        break;
                    }
                }
                let frame = if call.is_cancelled() {
                    Frame::failed(call.guid(), FailurePayload::Cancelled)
                } else {
                    match started.task.await {
                        Ok(Ok(output)) => match (wire.encode_output)(output) {
                            Ok(output) => Frame::finished(call.guid(), output),
                            Err(error) => Frame::failed(
                                call.guid(),
                                FailurePayload::Dispatch { error },
                            ),
                        },
                        Ok(Err(failure)) => Frame::failed(call.guid(), payload(failure, wire)),
                        Err(error) if error.is_cancelled() => {
                            Frame::failed(call.guid(), FailurePayload::Cancelled)
                        }
                        Err(_) => Frame::failed(
                            call.guid(),
                            FailurePayload::Dispatch {
                                error: DispatchError::Panicked {
                                    operation: O::ID.to_owned(),
                                },
                            },
                        ),
                    }
                };
                let _ = call.finish(frame).await;
            })
        });
        self.handlers.insert(O::ID, handler);
        Ok(())
    }

    pub(crate) fn handler(&self, id: &str) -> Option<Handler> {
        self.handlers.get(id).cloned()
    }
}

impl<P> std::fmt::Debug for Registry<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Registry")
            .field("operations", &self.ids())
            .finish_non_exhaustive()
    }
}

impl<P: Clone> Clone for Registry<P> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            handlers: self.handlers.clone(),
        }
    }
}

/// Where one erased call reports its events and observes cancellation.
pub(crate) struct Call {
    guid: Uuid,
    cancel: Arc<AtomicBool>,
    frames: mpsc::Sender<Frame>,
}

impl Call {
    pub(crate) fn new(guid: Uuid, cancel: Arc<AtomicBool>, frames: mpsc::Sender<Frame>) -> Self {
        Self {
            guid,
            cancel,
            frames,
        }
    }

    pub(crate) fn guid(&self) -> Uuid {
        self.guid
    }

    pub(crate) fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// Queues one event frame; fails once the call is cancelled or the session stopped.
    pub(crate) async fn send(&self, frame: Frame) -> Result<(), Cancelled> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(Cancelled);
        }
        self.finish(frame).await
    }

    /// Queues the terminal frame, which must be reported even after a cancellation.
    pub(crate) async fn finish(&self, frame: Frame) -> Result<(), Cancelled> {
        self.frames.send(frame).await.map_err(|_| Cancelled)
    }
}

/// Wire form of an operation failure, keeping the operation error encoded.
fn payload<O: Operation>(failure: Failure<O::Error>, wire: Wire<O>) -> FailurePayload {
    match failure {
        Failure::Operation(error) => match (wire.encode_error)(error) {
            Ok(error) => FailurePayload::Operation { error },
            Err(error) => FailurePayload::Dispatch { error },
        },
        Failure::Dispatch(error) => FailurePayload::Dispatch { error },
        // Serve never reaches a connection of its own; treat one as an unusable protocol.
        Failure::Transport(error) => FailurePayload::Dispatch {
            error: DispatchError::Protocol {
                message: error.to_string(),
            },
        },
        Failure::Cancelled => FailurePayload::Cancelled,
    }
}

/// The registry refused an operation definition.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("operation ID must not be empty")]
    EmptyId,
    #[error("duplicate operation ID: {id}")]
    DuplicateId { id: &'static str },
    #[error("operation has no wire codec and cannot be served: {id}")]
    NotRemotable { id: &'static str },
}
