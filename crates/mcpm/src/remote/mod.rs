//! Frontend bridge over `mcpm-exec` for the one remaining legacy command handler.
//!
//! `server.mods.cache` still owns its JSON codec and registration until `flows::mods_cache`
//! replaces it. Every migrated command now dispatches through `mcpm-backend` operations and
//! never depends on this module.

pub(crate) mod client;
pub(crate) mod command;
