//! Typed launch preparation notifications; terminal text belongs to the frontend.

use crate::data::server::authlib;

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
    Authentication(authlib::Event<'a>),
}
