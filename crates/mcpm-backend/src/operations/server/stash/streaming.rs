//! Host streaming operations hold one stash lease and cooperatively stop on progress delivery failure.

use anyhow::Result;
use mcpm_exec::operation::{Operation, StreamClosed, StreamSender, StreamingOutput};
use mcpm_lib::local::{mc::server::stash::StashStore, mcmod::backup};

use super::*;

/// Create a snapshot on the owning host.
pub struct Create;
impl Operation for Create {
    const ID: &'static str = "server.stash.create";
    type Input = CreateInput;
    type Output = StreamingOutput<CreateEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let mut progress = Progress::new(sender.clone(), CreateEvent::Progress);
            let stash = tokio::task::spawn_blocking(move || {
                let server = crate::workspace::inspect_local(input.path)?;
                let mut store = StashStore::write(&server)?;
                let stash =
                    store.create(input.name, input.parallelism, |event| progress.emit(event))?;
                Ok::<_, anyhow::Error>(StashSnapshot::from(&stash))
            })
            .await??;
            sender.send(CreateEvent::Created(stash)).await?;
            Ok(())
        }))
    }
}

/// Restore a fully verified snapshot on the owning host.
pub struct Switch;
impl Operation for Switch {
    const ID: &'static str = "server.stash.switch";
    type Input = SwitchInput;
    type Output = StreamingOutput<SwitchEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let mut progress = Progress::new(sender.clone(), SwitchEvent::Progress);
            let outcome = tokio::task::spawn_blocking(move || {
                let server = crate::workspace::inspect_local(input.path)?;
                let mut store = StashStore::write(&server)?;
                let outcome = store.switch(
                    &(&input.selector).into(),
                    input.force,
                    input.parallelism,
                    |event| progress.emit(event),
                )?;
                Ok::<_, anyhow::Error>((StashSnapshot::from(&outcome.stash), outcome.changed))
            })
            .await??;
            sender
                .send(SwitchEvent::Switched {
                    stash: outcome.0,
                    changed: outcome.1,
                })
                .await?;
            Ok(())
        }))
    }
}

/// Compare the workspace without changing HEAD or snapshot storage.
pub struct Status;
impl Operation for Status {
    const ID: &'static str = "server.stash.status";
    type Input = StatusInput;
    type Output = StreamingOutput<StatusEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let mut progress = Progress::new(sender.clone(), StatusEvent::Progress);
            let status = tokio::task::spawn_blocking(move || {
                let server = crate::workspace::inspect_local(input.path)?;
                let store = StashStore::read(&server)?;
                let status = store.status(input.parallelism, |event| progress.emit(event))?;
                Ok::<_, anyhow::Error>(StatusComparison::from(&status))
            })
            .await??;
            sender.send(StatusEvent::Status(status)).await?;
            Ok(())
        }))
    }
}

/// Verify selected snapshots sequentially, preserving independent item failures.
pub struct Verify;
impl Operation for Verify {
    const ID: &'static str = "server.stash.verify";
    type Input = VerifyInput;
    type Output = StreamingOutput<VerifyEvent>;
    async fn run(input: Self::Input) -> Result<Self::Output> {
        Ok(StreamingOutput::new(move |sender| async move {
            let mut progress = Progress::new(sender, VerifyEvent::Progress);
            tokio::task::spawn_blocking(move || {
                let server = crate::workspace::inspect_local(input.path)?;
                let store = StashStore::read(&server)?;
                if let Err(error) = store.head() {
                    progress.send(VerifyEvent::HeadFailed(error.into()))?;
                }
                // Resolve before verification, retaining first-occurrence labels and input order.
                let selected = super::resolve(&store, input.selectors);
                for (selector, result) in selected {
                    let result = result.and_then(|stash| {
                        store.verify(
                            &mcpm_lib::local::mc::server::stash::Selector::Id(stash.id()),
                            input.parallelism,
                            |event| progress.emit(event),
                        )
                    });
                    // A failed progress send is cancellation, not a recoverable item failure.
                    if progress.closed {
                        return Err(StreamClosed.into());
                    }
                    progress.send(VerifyEvent::Item {
                        selector,
                        result: result
                            .map(|stash| StashSnapshot::from(&stash))
                            .map_err(Into::into),
                    })?;
                }
                Ok(())
            })
            .await?
        }))
    }
}

/// Synchronous callback bridge; the stream owns cancellation and the library owns file work.
struct Progress<T> {
    sender: StreamSender<T>,
    runtime: tokio::runtime::Handle,
    event: fn(ProgressEvent) -> T,
    last: Option<ProgressEvent>,
    closed: bool,
}
impl<T> Progress<T> {
    fn new(sender: StreamSender<T>, event: fn(ProgressEvent) -> T) -> Self {
        Self {
            sender,
            runtime: tokio::runtime::Handle::current(),
            event,
            last: None,
            closed: false,
        }
    }
    fn send(&mut self, event: T) -> Result<()> {
        self.runtime
            .block_on(self.sender.send(event))
            .map_err(|error| {
                self.closed = true;
                error.into()
            })
    }
    fn emit(&mut self, event: backup::Event) -> Result<()> {
        let event = match event {
            backup::Event::Progress {
                stage,
                completed,
                total,
            } => ProgressEvent {
                stage: stage.into(),
                completed: completed as u64,
                total: total as u64,
            },
            // Re-send the completed stage to observe cancellation at this callback too.
            backup::Event::StageDone(stage) => self
                .last
                .filter(|last| last.stage == stage.into())
                .unwrap_or(ProgressEvent {
                    stage: stage.into(),
                    completed: 0,
                    total: 0,
                }),
        };
        self.last = Some(event);
        self.send((self.event)(event))
    }
}
