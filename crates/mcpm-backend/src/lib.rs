//! mcpm-backend: Operations and mcpm data codec
//!
//! Defines all operations and codec for mcpm's own data.
//! Responsibilities: operation Input/Output/errors and run; registry() assembles all operations;
//! workspace registration and other mcpm data codec.
//! Does not handle: execution location, transport.

pub mod operations;
pub mod workspace;
