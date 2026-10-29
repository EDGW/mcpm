//! Shared operation-error shape: an operation-defined classification plus one diagnostic
//! snapshot that survives the wire unchanged.
//!
//! `kako-mc-lib` and `data/` keep their own `anyhow` context chains and typed errors. An
//! operation decides the [`OperationError::kind`] from those typed errors or from its own
//! checks; it never classifies by parsing a message.

use std::fmt;

use kako_mc_lib::error::ErrorReport;
use serde::{Deserialize, Serialize};

/// Failure of one host operation.
///
/// The kind is the operation's own classification; the diagnostic keeps the complete
/// context chain and independent branches. Both are serializable, so a remote failure
/// reaches the caller with the same information a local failure has.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "K: Serialize", deserialize = "K: Deserialize<'de>"))]
pub struct OperationError<K> {
    /// The operation's classification of this failure.
    pub kind: K,
    /// The complete error snapshot captured where the failure was understood.
    pub diagnostic: ErrorReport,
}

impl<K> OperationError<K> {
    /// Captures `error` with its full chain and branches.
    pub fn new(kind: K, error: &anyhow::Error) -> Self {
        Self {
            kind,
            diagnostic: ErrorReport::capture(error),
        }
    }

    /// Captures an error that already has a full `anyhow` chain through `context`.
    pub fn from_source(kind: K, error: impl Into<anyhow::Error>) -> Self {
        Self::new(kind, &error.into())
    }

    /// Rebuilds an `anyhow` error for rendering, preserving the recorded chain.
    pub fn into_error(self) -> anyhow::Error {
        self.diagnostic.into_error()
    }

    /// The diagnostic snapshot without consuming the failure.
    pub fn diagnostic(&self) -> &ErrorReport {
        &self.diagnostic
    }
}

impl<K> fmt::Display for OperationError<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic.message)
    }
}

impl<K: fmt::Debug> std::error::Error for OperationError<K> {}
