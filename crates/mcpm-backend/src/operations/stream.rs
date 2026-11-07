//! Bridges synchronous business work to bounded operation streams with cooperative cancellation.

use anyhow::Result;
use mcpm_exec::operation::{StreamSender, StreamingOutput};

pub(crate) fn blocking_stream<T, F>(work: F) -> StreamingOutput<T>
where
    T: Send + 'static,
    F: FnOnce(Sender<T>) -> Result<()> + Send + 'static,
{
    StreamingOutput::new(move |sender| async move {
        let sender = Sender {
            sender,
            runtime: tokio::runtime::Handle::current(),
        };
        tokio::task::spawn_blocking(move || work(sender)).await?
    })
}

pub(crate) struct Sender<T> {
    sender: StreamSender<T>,
    runtime: tokio::runtime::Handle,
}
impl<T> Sender<T> {
    pub(crate) fn checkpoint(&self) -> Result<()> {
        self.sender.checkpoint().map_err(Into::into)
    }
    pub(crate) fn send(&self, item: T) -> Result<()> {
        self.checkpoint()?;
        self.runtime.block_on(self.sender.send(item))?;
        Ok(())
    }
}
