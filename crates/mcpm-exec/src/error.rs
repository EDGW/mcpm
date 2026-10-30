//! Failures a caller observes: dispatch and transport classifications, cancellation, and
//! the operation error carried through untouched.
//!
//! Exec produces [`DispatchError`], [`TransportError`] and [`Cancelled`] only. An operation
//! failure is never inspected here: [`Failure::Operation`] holds the operation's own error
//! and the caller classifies it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Failure of one call.
#[derive(Debug, Error)]
pub enum Failure<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    /// The operation failed; the value crosses exec unchanged.
    #[error(transparent)]
    Operation(E),
    /// The executor could not route, encode or decode the call.
    #[error(transparent)]
    Dispatch(#[from] DispatchError),
    /// The connection to the executing process failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// The call was cancelled by dropping its handle or by closing the connection.
    #[error("call cancelled")]
    Cancelled,
}

/// Failure classification for a call whose operation error is not decoded yet.
///
/// Serve hands this form to the wire; the caller resolves [`CallFailure::Operation`] with
/// the operation's own error decoder.
#[derive(Clone, Debug, PartialEq)]
pub enum CallFailure {
    /// The operation's error, still encoded.
    Operation(Value),
    Dispatch(DispatchError),
    Transport(TransportError),
    Cancelled,
}

impl std::fmt::Display for CallFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Operation(_) => formatter.write_str("the operation failed"),
            Self::Dispatch(error) => error.fmt(formatter),
            Self::Transport(error) => error.fmt(formatter),
            Self::Cancelled => Cancelled.fmt(formatter),
        }
    }
}

impl std::error::Error for CallFailure {}

/// Cooperative cancellation signalled by [`crate::Context::checkpoint`] and its emitter.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("call cancelled")]
pub struct Cancelled;

/// The executor could not route a call to an operation.
#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DispatchError {
    /// The executing process registered no operation with this identifier.
    #[error("operation is not registered: {operation}")]
    UnknownOperation { operation: String },
    /// The wire codec rejected the input before the operation started.
    #[error("invalid input for operation {operation}: {message}")]
    InvalidInput { operation: String, message: String },
    /// The wire codec rejected an event the operation emitted.
    #[error("invalid event for operation {operation}: {message}")]
    InvalidEvent { operation: String, message: String },
    /// The wire codec rejected the output the operation returned.
    #[error("invalid output for operation {operation}: {message}")]
    InvalidOutput { operation: String, message: String },
    /// The wire codec rejected the operation error carried by a failed call.
    #[error("invalid error for operation {operation}: {message}")]
    InvalidError { operation: String, message: String },
    /// A host operation without a wire codec was dispatched to a remote target.
    #[error("operation cannot cross a process boundary: {operation}")]
    NotRemotable { operation: String },
    /// A frame violated the protocol.
    #[error("protocol violation: {message}")]
    Protocol { message: String },
    /// The operation panicked; the executor reports it instead of the process dying.
    #[error("operation panicked: {operation}")]
    Panicked { operation: String },
}

/// The connection used to reach an executing process failed.
///
/// Causes are folded into the message so the value stays cheap to clone for every call
/// that was in flight when the connection failed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    /// The remote process could not be started.
    #[error("failed to start the remote session: {message}")]
    Start { message: String },
    /// The connection needs interactive authentication, which this transport never performs.
    #[error("remote authentication requires interaction: {message}")]
    Authentication { message: String },
    /// Reading or writing the protocol pipe failed.
    #[error("protocol pipe failure: {message}")]
    Io { message: String },
    /// The connection ended while calls were still possible.
    #[error("remote session ended: {message}")]
    Closed { message: String },
    /// The remote serve process exited.
    #[error("remote serve process exited: {message}")]
    Exited { message: String },
}
