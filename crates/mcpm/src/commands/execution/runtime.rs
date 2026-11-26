//! Command runtime that cancels asynchronous tasks without waiting for blocking work on exit.

use std::ops::Deref;

use anyhow::{Context, Result};

#[derive(Debug)]
pub(in crate::commands) struct Runtime(Option<tokio::runtime::Runtime>);

impl Runtime {
    pub(in crate::commands) fn new() -> Result<Self> {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map(|runtime| Self(Some(runtime)))
            .context("failed to create command runtime")
    }

    /// Waits up to `timeout` for in-flight blocking work (such as cancelling a child
    /// process tree and cleaning up temporary staging files) before exiting.
    pub(in crate::commands) fn shutdown_timeout(mut self, timeout: std::time::Duration) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_timeout(timeout);
        }
    }
}

impl Deref for Runtime {
    type Target = tokio::runtime::Runtime;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("command runtime is alive until drop")
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            tracing::debug!("Shutting down command runtime without waiting for blocking work");
            // Tokio cannot abort an already-running spawn_blocking hash/fingerprint
            // or DNS lookup. The default runtime drop would wait for it indefinitely.
            runtime.shutdown_background();
        }
    }
}
