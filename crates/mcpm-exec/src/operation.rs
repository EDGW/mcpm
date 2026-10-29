//! Operation contract: the dispatchable unit, its placement, its run context and its wire codec.
//!
//! The executor knows no concrete operation, environment or business type. An operation
//! declares where it runs ([`Placement`]) and which environment it needs; the environment
//! is supplied by a provider the caller owns, so a missing environment fails at compile time.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
};

use serde_json::Value;
use tokio::sync::mpsc;

use crate::error::{Cancelled, DispatchError};

/// The smallest dispatchable unit; placement is fixed by registration, not by the caller.
pub trait Operation: Sized + Send + Sync + 'static {
    /// Stable identifier; serve resolves requests by this string.
    const ID: &'static str;

    /// Where the operation may execute; only registration sets this.
    const PLACEMENT: Placement;

    /// JSON codec used when the operation crosses a process boundary.
    ///
    /// Registration macros for host operations provide it; surface operations keep the
    /// default and therefore cannot leave the calling process.
    const WIRE: Option<Wire<Self>> = None;

    /// Process-local state supplied by the executor's provider.
    type Env: Send + Sync + 'static;

    type Input: Send;
    type Event: Send;
    type Output: Send;

    /// The operation's own failure classification; the executor passes it through unchanged.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Runs one call; events are reported through `cx` and the output is returned once.
    fn run(
        input: Self::Input,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send;
}

/// Where an operation runs; set only by registration macros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Always in the calling process; the target is ignored and never connects.
    Surface,
    /// In the calling process for a local target, or on the host for a remote one.
    Host,
}

/// Supplies the environment an operation declares, one value per call.
///
/// The provider belongs to the process entry point; exec never learns which concrete
/// environment it hands out.
pub trait Provide<E: Send + Sync + 'static>: Send + Sync + 'static {
    fn provide(&self) -> E;
}

/// Per-call capability handed to [`Operation::run`].
///
/// Cancellation is cooperative: the executor observes its own token, so an operation
/// that cannot be interrupted (a blocking closure) checks [`Context::checkpoint`]
/// between steps.
pub struct Context<'call, O: Operation> {
    env: &'call O::Env,
    cancel: Arc<AtomicBool>,
    events: mpsc::Sender<O::Event>,
    blocking_events: SyncSender<O::Event>,
}

impl<O: Operation> Context<'_, O> {
    pub(crate) fn new(
        env: &O::Env,
        cancel: Arc<AtomicBool>,
        events: mpsc::Sender<O::Event>,
        blocking_events: SyncSender<O::Event>,
    ) -> Context<'_, O> {
        Context {
            env,
            cancel,
            events,
            blocking_events,
        }
    }

    /// The environment supplied by the executor's provider.
    pub fn env(&self) -> &O::Env {
        self.env
    }

    /// Fails once the call was cancelled, so the operation can stop between steps.
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        if self.cancel.load(Ordering::Acquire) {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }

    /// Reports one event, waiting while the caller's bounded queue is full.
    pub async fn emit(&self, event: O::Event) -> Result<(), Cancelled> {
        self.checkpoint()?;
        self.events.send(event).await.map_err(|_| Cancelled)
    }

    /// Runs synchronous work on the blocking pool; the closure itself cannot be interrupted.
    ///
    /// The call's current span is entered on that thread, so records emitted by synchronous
    /// code still belong to this call.
    pub async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> T {
        let span = tracing::Span::current();
        tokio::task::spawn_blocking(move || span.in_scope(work))
            .await
            .expect("operation blocking task was cancelled")
    }

    /// Cloneable event sink for synchronous code reached through [`Context::blocking`].
    pub fn emitter(&self) -> Emitter<O::Event> {
        Emitter {
            events: self.blocking_events.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

/// Event sink for synchronous code reached through [`Context::blocking`].
///
/// Events enter a bounded synchronous queue drained into the call's bounded event queue by
/// a blocking task, so a producer inside another runtime never needs a worker to progress.
/// The queue is bounded, so a slow caller still slows the operation down.
#[derive(Debug)]
pub struct Emitter<E> {
    events: SyncSender<E>,
    cancel: Arc<AtomicBool>,
}

impl<E> Emitter<E> {
    /// Fails once the call was cancelled.
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        if self.cancel.load(Ordering::Acquire) {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }

    /// Reports one event, waiting while either bounded queue is full.
    ///
    /// Call it only from blocking code: a producer that blocks on a runtime worker would
    /// keep the consumer from running.
    pub fn emit(&self, event: E) -> Result<(), Cancelled> {
        self.checkpoint()?;
        self.events.send(event).map_err(|_| Cancelled)
    }
}

impl<E> Clone for Emitter<E> {
    fn clone(&self) -> Self {
        Self {
            events: self.events.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

/// JSON codec for an operation that crosses a process boundary.
///
/// Serve encodes inputs, events, outputs and errors; the caller decodes events, outputs
/// and errors and encodes inputs. Registration macros construct it where the serde bounds
/// hold, so [`Operation`] itself stays free of serialization requirements.
pub struct Wire<O: Operation> {
    pub(crate) encode_input: fn(O::Input) -> Result<Value, DispatchError>,
    pub(crate) decode_input: fn(Value) -> Result<O::Input, DispatchError>,
    pub(crate) encode_event: fn(O::Event) -> Result<Value, DispatchError>,
    pub(crate) decode_event: fn(Value) -> Result<O::Event, DispatchError>,
    pub(crate) encode_output: fn(O::Output) -> Result<Value, DispatchError>,
    pub(crate) decode_output: fn(Value) -> Result<O::Output, DispatchError>,
    pub(crate) encode_error: fn(O::Error) -> Result<Value, DispatchError>,
    pub(crate) decode_error: fn(Value) -> Result<O::Error, DispatchError>,
}

impl<O: Operation> Clone for Wire<O> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<O: Operation> Copy for Wire<O> {}

impl<O: Operation> Wire<O> {
    /// Builds the codec; registration macros pass one function per direction.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        encode_input: fn(O::Input) -> Result<Value, DispatchError>,
        decode_input: fn(Value) -> Result<O::Input, DispatchError>,
        encode_event: fn(O::Event) -> Result<Value, DispatchError>,
        decode_event: fn(Value) -> Result<O::Event, DispatchError>,
        encode_output: fn(O::Output) -> Result<Value, DispatchError>,
        decode_output: fn(Value) -> Result<O::Output, DispatchError>,
        encode_error: fn(O::Error) -> Result<Value, DispatchError>,
        decode_error: fn(Value) -> Result<O::Error, DispatchError>,
    ) -> Self {
        Self {
            encode_input,
            decode_input,
            encode_event,
            decode_event,
            encode_output,
            decode_output,
            encode_error,
            decode_error,
        }
    }
}
