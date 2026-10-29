//! Content-cache operations.
//!
//! Cache directories and their metadata belong to [`crate::data::cache`]; these operations
//! interpret one request, apply the stash protection policy and report per-entry results.
//! Remaining cache commands move here in a later step.

pub mod ident;

pub use ident::{
    IdentEntry, IdentWrite, IdentWriteEvent, IdentWriteInput, IdentWriteKind, IdentWriteStatus,
    IdentWriteSummary,
};
