//! Frontend bridge over `mcpm-exec` for the legacy command handlers.
//!
//! The handlers, their JSON codecs and their registration still belong to the commands
//! until they become host operations. This module adapts them to the executor contract and
//! converts executor failures back into the frontend's error model.

pub(crate) mod client;
pub(crate) mod command;
pub(crate) mod registry;
