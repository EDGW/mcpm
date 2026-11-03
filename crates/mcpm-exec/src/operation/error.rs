//! Errors of the operation protocol.

// Imports

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// An error that crossed the session, with its whole cause chain.
///
/// Built from an [`anyhow::Error`] on the executing side; on the calling side it is the source of
/// the returned `anyhow::Error`, so `{:#}` and [`anyhow::Error::chain`] show every cause.
#[derive(Debug, Clone, Serialize, Deserialize, Error)]
#[serde(deny_unknown_fields)]
#[error("{message}")]
pub struct RemoteError {
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[source]
    source: Option<Box<RemoteError>>,
}

impl RemoteError {
    /// Top-level message, without causes.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<&anyhow::Error> for RemoteError {
    fn from(error: &anyhow::Error) -> Self {
        let mut messages: Vec<String> = error.chain().map(ToString::to_string).collect();
        // `chain` always yields the error itself, so the vector is non-empty.
        let mut remote = Self {
            message: messages.pop().unwrap_or_default(),
            source: None,
        };
        while let Some(message) = messages.pop() {
            remote = Self {
                message,
                source: Some(Box::new(remote)),
            };
        }
        remote
    }
}

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
