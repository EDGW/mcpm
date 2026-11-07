//! Execution layer shared by mcpm and mcpmd: messaging over byte streams and remote operations
//! on top of it.

pub mod daemon;
pub mod endpoint;
pub mod error;
pub mod messaging;
pub mod operation;
pub mod service;
