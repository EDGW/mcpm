//! Typed in-process execution: the operation runs as a task and events travel unencoded.
//!
//! Both local dispatch and serve use this path, so an operation observes the same context
//! whether its caller is in this process or behind the protocol. Events keep one bounded
//! queue; synchronous producers reach it through a bounded bridge so a handler that runs
//! inside another runtime never needs a runtime worker to make progress.

use std::sync::{
    Arc,
    atomic::AtomicBool,
    mpsc::{Receiver, sync_channel},
};

use tokio::{sync::mpsc, task::JoinHandle};
use tracing::Instrument;

use crate::{
    EVENT_CAPACITY,
    error::Failure,
    operation::{Context, Operation},
};

/// One running call: its event stream and the task that produces the result.
pub(crate) struct Started<O: Operation> {
    pub(crate) events: mpsc::Receiver<O::Event>,
    pub(crate) task: JoinHandle<Result<O::Output, Failure<O::Error>>>,
}

/// Runs one operation on the current runtime; the caller owns the cancellation token.
pub(crate) fn start<O: Operation>(
    env: O::Env,
    input: O::Input,
    cancel: Arc<AtomicBool>,
) -> Started<O> {
    let (events, receiver) = mpsc::channel(EVENT_CAPACITY);
    let (blocking, buffered) = sync_channel::<O::Event>(EVENT_CAPACITY);
    bridge(buffered, events.clone());
    // Keep the caller's span, so records of this call stay attributable to it.
    let span = tracing::Span::current();
    let task = tokio::spawn(
        async move {
            let cx = Context::new(&env, cancel, events, blocking);
            O::run(input, cx).await.map_err(Failure::Operation)
        }
        .instrument(span),
    );
    Started {
        events: receiver,
        task,
    }
}

/// Moves synchronously produced events into the call's bounded queue in order.
///
/// The bridge runs on the blocking pool, so a producer blocked inside a foreign runtime
/// never stops the consumer from draining the call's queue.
fn bridge<E: Send + 'static>(buffered: Receiver<E>, events: mpsc::Sender<E>) {
    tokio::task::spawn_blocking(move || {
        for event in buffered {
            if events.blocking_send(event).is_err() {
                break;
            }
        }
    });
}
