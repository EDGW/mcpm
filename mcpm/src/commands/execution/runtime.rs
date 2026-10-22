//! Command runtime that cancels asynchronous tasks without waiting for blocking work on exit.

use std::ops::Deref;

use anyhow::{Context, Result};

#[derive(Debug)]
pub(in crate::commands) struct Runtime(Option<tokio::runtime::Runtime>);

impl Runtime {
    pub(in crate::commands) fn new() -> Result<Self> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|runtime| Self(Some(runtime)))
            .context("failed to create command runtime")
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
