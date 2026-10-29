//! General Minecraft capabilities: installations, mod metadata, dependencies,
//! remote mod sources, Java invocations and downloads.
//!
//! The library never reads or writes mcpm's own data directory; callers own
//! that layout and feed it in explicitly.

pub mod download;
pub mod error;
pub mod local;
pub mod source;

pub mod blocking;
