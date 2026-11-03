//! Calling side: requests an operation from the peer.

// Imports

use anyhow::{Context, Result};

use super::protocol::{Request, Response};
use super::{Operation, OperationOutput, ProtocolError};
use crate::messaging::Session;

/// Runs `O` on the peer of `session` and returns its output.
///
/// Remote failures keep their cause chain under the context ``operation `ID` failed``. Dropping
/// the returned future cancels the operation on the peer.
pub async fn call<O: Operation>(session: &Session, input: &O::Input) -> Result<O::Output> {
    request::<O>(session, input)
        .await
        .with_context(|| format!("operation `{}` failed", O::ID))
}

async fn request<O: Operation>(session: &Session, input: &O::Input) -> Result<O::Output> {
    let mut channel = session.open();
    channel.send(Request::encode(O::ID, input)?)?;
    let body = channel
        .recv()
        .await?
        .ok_or(ProtocolError::Missing("response"))?;
    match Response::decode(body)? {
        Response::Ok(value) => O::Output::receive(value, channel),
        Response::Error(error) => Err(error.into()),
    }
}
