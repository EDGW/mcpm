//! Operations, business functions and the sole codecs for mcpm's own data.
//!
//! `mcpm-backend` owns every operation: surface operations run only in the calling process
//! and may hold providers or credentials; host operations run wherever the target is and
//! only ever see [`HostEnv`]. Each mcpm-specific format is encoded and decoded in exactly
//! one module under [`data`], and general Minecraft behaviour stays in `kako-mc-lib`.
//! The crate also exposes the environment provider and the process executor/session types
//! every entry point uses to dispatch those operations.

// The operation trait declares a desugared `impl Future`, so an `async fn` impl cannot
// satisfy it; the lint is therefore expected in every operation.
#![allow(clippy::manual_async_fn)]

use mcpm_exec::{Executor, Provide, Registry, RegistryError, Settings};

pub mod cache;
pub mod data;
pub mod env;
pub mod error;
pub mod mods;
pub mod register;
pub mod scope;
pub mod server;
pub mod source;
pub mod workspace;

pub use env::{HostEnv, SurfaceEnv};
pub use workspace::{ConnectionSpec, RemoteView, ResolvedWorkspace};

/// Environment provider of this build, supplied to one process-wide executor.
///
/// Surface operations receive the HTTP client; host operations receive no state at all, so
/// provider access is impossible from code that may run on another machine.
#[derive(Clone, Debug)]
pub struct Providers {
    surface: SurfaceEnv,
}

impl Providers {
    pub fn new(surface: SurfaceEnv) -> Self {
        Self { surface }
    }
}

impl Provide<SurfaceEnv> for Providers {
    fn provide(&self) -> SurfaceEnv {
        self.surface.clone()
    }
}

impl Provide<HostEnv> for Providers {
    fn provide(&self) -> HostEnv {
        HostEnv
    }
}

/// A caller's target-bound dispatch context for this build's environments.
pub type Session = mcpm_exec::Session<Providers>;

/// Builds the process executor; the entry point owns its logging settings.
pub fn executor(surface: SurfaceEnv, settings: Settings) -> Executor<Providers> {
    Executor::new(Providers::new(surface), settings)
}

/// Builds the serve registry from every host operation declared by this build.
pub fn register_host_operations(
    providers: Providers,
) -> Result<Registry<Providers>, RegistryError> {
    register::register_host_operations(providers)
}
