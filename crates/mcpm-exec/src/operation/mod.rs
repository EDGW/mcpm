//! Remote operations over a messaging session.
//!
#![doc = include_str!("../docs/operation.md")]

// Imports

use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::messaging::Channel;

// Re-exported Modules

mod call;
pub use call::*;

mod error;
pub use error::*;

mod protocol;
pub use protocol::Reply;

mod serve;
pub use serve::*;

mod streaming;
pub use streaming::*;

/// An operation that the peer of a session can run.
///
/// The executing side registers it in a [`Registry`]; the calling side runs it with [`call`].
pub trait Operation: 'static {
    /// Name on the wire, unique within a registry.
    const ID: &'static str;
    type Input: Serialize + DeserializeOwned + Send + 'static;
    type Output: OperationOutput;

    /// Runs the operation on the executing side. Dropped when the caller cancels.
    fn run(input: Self::Input) -> impl Future<Output = Result<Self::Output>> + Send;
}

/// Result of an operation and the rest of its channel.
///
/// The operation protocol only carries the first reply; the output type owns the channel after
/// it. Every serde type is an output that finishes the channel with its value.
/// [`StreamingOutput`] keeps the channel for its items.
pub trait OperationOutput: Sized + Send + 'static {
    /// Executing side: sends the result through `reply`, then uses the channel as needed.
    fn deliver(self, reply: Reply) -> impl Future<Output = Result<()>> + Send;

    /// Calling side: builds the output from the first reply and takes over its channel.
    fn receive(value: Value, channel: Channel) -> Result<Self>;
}

impl<T: Serialize + DeserializeOwned + Send + 'static> OperationOutput for T {
    async fn deliver(self, reply: Reply) -> Result<()> {
        reply.finish(&self)
    }

    fn receive(value: Value, _channel: Channel) -> Result<Self> {
        Ok(serde_json::from_value(value).map_err(|error| ProtocolError::Invalid("output", error))?)
    }
}
