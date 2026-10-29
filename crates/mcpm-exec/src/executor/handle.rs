//! Call handle: the event stream, the terminal result, and cancellation on drop.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::{sync::mpsc, task::JoinHandle};

use crate::{
    error::{DispatchError, Failure},
    operation::Operation,
};

/// The driver task that produces one call's terminal result.
pub(crate) type Task<O> = JoinHandle<Result<<O as Operation>::Output, Failure<<O as Operation>::Error>>>;

/// One running call owned by its caller.
///
/// Events arrive in order through [`Handle::next`]; [`Handle::finish`] yields the output or
/// the failure. Dropping the handle cancels the call.
pub struct Handle<O: Operation> {
    events: mpsc::Receiver<O::Event>,
    cancel: Arc<AtomicBool>,
    task: Option<Task<O>>,
}

impl<O: Operation> std::fmt::Debug for Handle<O> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Handle")
            .field("running", &self.task.is_some())
            .finish_non_exhaustive()
    }
}

impl<O: Operation> Handle<O> {
    pub(crate) fn new(
        events: mpsc::Receiver<O::Event>,
        task: Task<O>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            events,
            cancel,
            task: Some(task),
        }
    }

    /// The next event, or `None` once the operation emitted all of them.
    pub async fn next(&mut self) -> Option<O::Event> {
        self.events.recv().await
    }

    /// The terminal result; events not yet received are discarded.
    ///
    /// The event queue stays bounded, so this drains it while waiting: an operation that
    /// emitted more events than the queue holds must still be able to finish.
    pub async fn finish(mut self) -> Result<O::Output, Failure<O::Error>> {
        let task = self.task.take().expect("handle is finished once");
        tokio::pin!(task);
        let result = loop {
            tokio::select! {
                result = &mut task => break result,
                event = self.events.recv() => {
                    if event.is_none() {
                        break task.await;
                    }
                }
            }
        };
        match result {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Err(Failure::Cancelled),
            Err(_) => Err(Failure::Dispatch(DispatchError::Panicked {
                operation: O::ID.to_owned(),
            })),
        }
    }
}

impl<O: Operation> Drop for Handle<O> {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            self.cancel.store(true, Ordering::Release);
            task.abort();
        }
    }
}
