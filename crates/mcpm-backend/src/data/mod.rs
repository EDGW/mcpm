//! Sole codecs for mcpm's on-disk data and the resources that interpret it.
//!
//! Domains nest by ownership: `server/` holds server registration and everything
//! derived from it, `mods/` holds mod-file formats, `cache/` and `runtime/` hold
//! the shared content cache and the resources bound to it.

pub mod cache;
pub mod mods;
pub mod runtime;
pub mod server;
pub mod workspace;
