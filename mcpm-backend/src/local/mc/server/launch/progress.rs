//! Typed launch preparation notifications; terminal text belongs to the frontend.

use crate::local::mc::server::authlib_injector;

/// Actual preparation boundaries, without an estimated overall percentage.
#[derive(Debug, Clone, Copy)]
pub enum Stage {
    Configuration,
    LocalChecks,
    JavaCapabilities,
    Ready,
}

/// Synchronous observation of preparation, including nested authentication work.
/// Observer errors abort preparation; returning a plan never implies Minecraft readiness.
#[derive(Debug, Clone, Copy)]
pub enum Event<'a> {
    Stage(Stage),
    Authentication(authlib_injector::Event<'a>),
}
