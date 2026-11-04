//! Bridges synchronous cache work to bounded operation streams with cooperative cancellation.

use anyhow::Result;
use mcpm_exec::operation::{StreamClosed, StreamSender, StreamingOutput};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn blocking_stream<T, F>(work: F) -> StreamingOutput<T>
where
    T: Send + 'static,
    F: FnOnce(Sender<T>) -> Result<()> + Send + 'static,
{
    StreamingOutput::new(move |sender| async move {
        let cancelled = Arc::new(AtomicBool::new(false));
        let _guard = Cancel(cancelled.clone());
        let sender = Sender {
            sender,
            runtime: tokio::runtime::Handle::current(),
            cancelled,
        };
        tokio::task::spawn_blocking(move || work(sender)).await?
    })
}

pub(super) struct Sender<T> {
    sender: StreamSender<T>,
    runtime: tokio::runtime::Handle,
    cancelled: Arc<AtomicBool>,
}
impl<T> Sender<T> {
    pub(super) fn checkpoint(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(StreamClosed.into());
        }
        Ok(())
    }
    pub(super) fn send(&self, item: T) -> Result<()> {
        self.checkpoint()?;
        self.runtime.block_on(self.sender.send(item))?;
        Ok(())
    }
}
struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
