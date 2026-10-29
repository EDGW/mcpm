//! Executor and target-bound session.
//!
//! [`Executor`] holds the environment provider and the client settings of the process.
//! [`Session`] binds that executor to one [`Target`] and starts calls: a surface operation,
//! or a host operation on a local target, runs in-process; a host operation on a remote
//! target connects lazily, so a flow of surface operations never opens a connection.

use std::sync::{
    Arc,
    atomic::AtomicBool,
};

use tokio::sync::{Mutex, mpsc};
use tracing::debug;

use crate::{
    error::{CallFailure, DispatchError, Failure, TransportError},
    operation::{Operation, Placement},
    protocol::{LogLevel, LogRecord, Status},
    registry::Registry,
    transport::{inprocess, session},
};

mod handle;
mod target;

pub use crate::operation::Provide;
pub use handle::Handle;
pub use target::Target;

/// Client settings shared by every remote session of the process.
#[derive(Clone)]
pub struct Settings {
    build: String,
    level: LogLevel,
    logs: Arc<dyn Fn(LogRecord) + Send + Sync>,
}

impl Settings {
    /// `logs` receives the remote records of every call; it renders them into local logging.
    pub fn new(
        build: impl Into<String>,
        level: LogLevel,
        logs: Arc<dyn Fn(LogRecord) + Send + Sync>,
    ) -> Self {
        Self {
            build: build.into(),
            level,
            logs,
        }
    }

    pub fn build(&self) -> &str {
        &self.build
    }

    pub fn level(&self) -> LogLevel {
        self.level
    }

    fn session(&self) -> session::Options {
        session::Options::new(self.build.clone(), self.level, self.logs.clone())
    }
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Settings")
            .field("build", &self.build)
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

/// Process-wide dispatch context: the environment provider plus the client settings.
pub struct Executor<P> {
    provider: P,
    settings: Settings,
}

impl<P> Executor<P> {
    pub fn new(provider: P, settings: Settings) -> Self {
        Self { provider, settings }
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Builds the serve-side registry for this process's host operations.
    pub fn registry(&self) -> Registry<P>
    where
        P: Clone,
    {
        Registry::new(self.provider.clone())
    }
}

impl<P: Clone> Clone for Executor<P> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            settings: self.settings.clone(),
        }
    }
}

impl<P> std::fmt::Debug for Executor<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Executor")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// One target-bound dispatch context.
pub struct Session<P> {
    executor: Executor<P>,
    target: Target,
    remote: Mutex<Option<Arc<session::Session>>>,
}

impl<P> Session<P> {
    pub fn new(executor: Executor<P>, target: Target) -> Self {
        Self {
            executor,
            target,
            remote: Mutex::new(None),
        }
    }

    pub fn executor(&self) -> &Executor<P> {
        &self.executor
    }

    pub fn target(&self) -> &Target {
        &self.target
    }
}

impl<P: Clone + Send + Sync + 'static> Session<P> {
    /// Starts one call; must be awaited inside a Tokio runtime.
    pub async fn start<O>(&self, input: O::Input) -> Result<Handle<O>, Failure<O::Error>>
    where
        O: Operation,
        P: Provide<O::Env>,
    {
        if matches!(O::PLACEMENT, Placement::Surface) || !self.target.is_remote() {
            return Ok(self.start_local(input));
        }
        self.start_remote(input).await
    }

    fn start_local<O>(&self, input: O::Input) -> Handle<O>
    where
        O: Operation,
        P: Provide<O::Env>,
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let started = inprocess::start::<O>(self.executor.provider.provide(), input, cancel.clone());
        Handle::new(started.events, started.task, cancel)
    }

    async fn start_remote<O>(&self, input: O::Input) -> Result<Handle<O>, Failure<O::Error>>
    where
        O: Operation,
    {
        let Some(wire) = O::WIRE else {
            return Err(Failure::Dispatch(DispatchError::NotRemotable {
                operation: O::ID.to_owned(),
            }));
        };
        let input = (wire.encode_input)(input).map_err(Failure::Dispatch)?;
        let pending = self
            .connect()
            .await
            .map_err(connect_failure::<O>)?
            .submit(O::ID, input)
            .map_err(|failure| failure_of(failure, wire))?;
        Ok(remote_handle(
            pending,
            wire,
            Arc::new(AtomicBool::new(false)),
        ))
    }

    /// One connection per session, opened on the first remote call and reused afterwards.
    ///
    /// Handshake failures keep the classification the session produced: a rejected build or a
    /// malformed frame is a dispatch failure of the caller's call, not a transport failure.
    async fn connect(&self) -> Result<Arc<session::Session>, CallFailure> {
        let mut open = self.remote.lock().await;
        if let Some(session) = open.as_ref()
            && session.is_open()
        {
            return Ok(session.clone());
        }
        let Some(connection) = self.target.connection().cloned() else {
            return Err(CallFailure::Transport(TransportError::Start {
                message: "the session target is not remote".to_owned(),
            }));
        };
        debug!(
            destination = connection.destination().unwrap_or_default(),
            "Opening the remote session"
        );
        let options = self.executor.settings.session();
        let started = tokio::task::spawn_blocking(move || {
            let mut command = connection.command().map_err(|error| {
                CallFailure::Transport(TransportError::Start {
                    message: error.to_string(),
                })
            })?;
            session::Session::start(&mut command, options)
        })
        .await
        .map_err(|error| {
            CallFailure::Transport(TransportError::Start {
                message: error.to_string(),
            })
        })?;
        let session = Arc::new(started?);
        *open = Some(session.clone());
        Ok(session)
    }
}

impl<P> std::fmt::Debug for Session<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("executor", &self.executor)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

/// Turns one remote call into a handle: events are decoded as they arrive.
fn remote_handle<O: Operation>(
    pending: session::Pending,
    wire: crate::operation::Wire<O>,
    cancel: Arc<AtomicBool>,
) -> Handle<O> {
    let (events, receiver) = mpsc::channel(crate::EVENT_CAPACITY);
    let driver = tokio::spawn(async move {
        let mut pending = pending;
        loop {
            let frame = pending
                .next()
                .await
                .map_err(|failure| failure_of(failure, wire))?;
            match frame.status() {
                Status::Data => match (wire.decode_event)(frame.into_contents()) {
                    Ok(event) => {
                        if events.send(event).await.is_err() {
                            return Err(Failure::Cancelled);
                        }
                    }
                    Err(error) => return Err(Failure::Dispatch(error)),
                },
                Status::Finished => {
                    return (wire.decode_output)(frame.into_contents()).map_err(Failure::Dispatch);
                }
                status => {
                    return Err(Failure::Dispatch(DispatchError::Protocol {
                        message: format!("unexpected reply frame: {status:?}"),
                    }));
                }
            }
        }
    });
    Handle::new(receiver, driver, cancel)
}

/// Maps a connection failure without hiding its classification.
///
/// The handshake cannot produce an operation error; one would mean the peer answered a
/// request that was never sent, which is a protocol violation rather than a transport fault.
fn connect_failure<O: Operation>(failure: CallFailure) -> Failure<O::Error> {
    match failure {
        CallFailure::Dispatch(error) => Failure::Dispatch(error),
        CallFailure::Transport(error) => Failure::Transport(error),
        CallFailure::Cancelled => Failure::Cancelled,
        CallFailure::Operation(error) => Failure::Dispatch(DispatchError::Protocol {
            message: format!("the session handshake carried an operation error: {error}"),
        }),
    }
}

fn failure_of<O: Operation>(
    failure: CallFailure,
    wire: crate::operation::Wire<O>,
) -> Failure<O::Error> {
    match failure {
        CallFailure::Operation(error) => match (wire.decode_error)(error) {
            Ok(error) => Failure::Operation(error),
            Err(error) => Failure::Dispatch(error),
        },
        CallFailure::Dispatch(error) => Failure::Dispatch(error),
        CallFailure::Transport(error) => Failure::Transport(error),
        CallFailure::Cancelled => Failure::Cancelled,
    }
}

#[cfg(test)]
mod tests;

