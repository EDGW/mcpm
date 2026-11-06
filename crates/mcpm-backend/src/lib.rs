//! mcpm-backend: Operations and mcpm data codec
//!
//! Defines all operations and codec for mcpm's own data.
//! Responsibilities: operation Input/Output/errors and run; registry() assembles all operations;
//! workspace codecs and Host-side shim lifecycle/IPC services.
//! Execution-location selection and SSH transport remain outside this crate.

pub mod operations;
pub mod server;
pub mod workspace;
