//! GUID-based session routing; command modules own their codecs and register via initcalls.

pub(crate) mod client;
pub(crate) mod command;
pub(crate) mod protocol;
pub(crate) mod registry;
pub(crate) mod server;
