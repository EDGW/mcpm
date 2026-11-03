//! Errors of the operation protocol.

// Imports

use thiserror::Error;

/// The peer broke the operation protocol.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("peer sent an invalid {0}")]
    Invalid(&'static str, #[source] serde_json::Error),
    #[error("channel ended before the {0}")]
    Missing(&'static str),
    #[error("peer sent a message the protocol does not expect")]
    Unexpected,
}
