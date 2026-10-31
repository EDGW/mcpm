//! Errors shared by sessions and channels.

// Imports

use std::io;
use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use super::FrameError;

/// Why a session stopped before the peer closed it cleanly.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("failed to read from the carrier")]
    Read(#[source] io::Error),
    #[error("failed to write to the carrier")]
    Write(#[source] io::Error),
    #[error("failed to encode a frame")]
    Encode(#[source] serde_json::Error),
    #[error("peer sent an invalid frame")]
    Protocol(#[source] FrameError),
}

/// Failure of a channel operation.
#[derive(Debug, Clone, Error)]
pub enum ChannelError {
    /// Either side finished the channel.
    #[error("channel {0} is finished")]
    Finished(Uuid),
    /// The session ended while the channel was open. The source is absent when the peer closed
    /// the session cleanly.
    #[error("session closed")]
    SessionClosed(#[source] Option<Arc<SessionError>>),
}
