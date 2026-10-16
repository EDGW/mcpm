//! Shared FML formats and the concrete Forge/NeoForge implementations.
//!
//! Shared readers receive format differences as data and never call a concrete
//! loader. Each loader owns its client/server entry points and error boundary.

mod client;
mod constraints;
mod dependency;
mod jarjar;
mod metadata;
mod server;
mod version;

pub mod forge;
pub mod neoforge;
