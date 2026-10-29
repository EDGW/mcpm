//! Legacy command registration and the serve entry point of this process.
//!
//! Handlers register from their own modules; the registry only knows erased JSON handlers.
//! Once handlers become host operations, registration moves to the backend and this module
//! disappears.

use std::sync::{LazyLock, Mutex};

use anyhow::{Context, Result};

use mcpm_exec::{Provide, Registry};

use super::command::{Command, Handler};

/// Environment provider of the adapter; legacy handlers need no process-local state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Providers;

impl Provide<()> for Providers {
    fn provide(&self) {}
}

static REGISTERED: LazyLock<Mutex<Registry<Providers>>> =
    LazyLock::new(|| Mutex::new(Registry::new(Providers)));

pub(crate) fn register<C: Command>() -> Result<()> {
    REGISTERED
        .lock()
        .expect("the serve registry is never poisoned")
        .register::<Handler<C>>()
        .map_err(anyhow::Error::new)
        .with_context(|| format!("failed to register remote command {}", C::ID))
}

/// Serves the registered handlers until the caller closes stdin.
pub(crate) async fn serve() -> Result<()> {
    let registry = REGISTERED
        .lock()
        .expect("the serve registry is never poisoned")
        .clone();
    mcpm_exec::serve::run(registry, crate::build_info::VERSION)
        .await
        .map_err(anyhow::Error::new)
        .context("remote serve session failed")
}
