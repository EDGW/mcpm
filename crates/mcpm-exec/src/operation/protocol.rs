//! Operation envelope: the request and the first reply on an operation channel.
//!
//! This module is the only place that encodes or decodes the envelope. Messages after the first
//! reply belong to the output type.

// Imports

use std::borrow::Cow;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ProtocolError, RemoteError};
use crate::messaging::Channel;

/// First message of an operation channel, sent by the caller.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub op: Cow<'static, str>,
    pub input: Value,
}

impl Request {
    pub fn encode(op: &'static str, input: &impl Serialize) -> Result<Value> {
        let input = serde_json::to_value(input).context("failed to encode operation input")?;
        Ok(serde_json::to_value(Self { op: Cow::Borrowed(op), input })?)
    }

    pub fn decode(body: Value) -> Result<Self, ProtocolError> {
        serde_json::from_value(body).map_err(|error| ProtocolError::Invalid("request", error))
    }
}

/// First reply of an operation channel, sent by the executing side.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Response {
    Ok(Value),
    Error(RemoteError),
}

impl Response {
    pub fn decode(body: Value) -> Result<Self, ProtocolError> {
        serde_json::from_value(body).map_err(|error| ProtocolError::Invalid("response", error))
    }

    fn encode(&self) -> Value {
        // Both variants hold JSON-compatible data with string keys.
        serde_json::to_value(self).expect("response is always valid JSON")
    }
}

/// Executing side of an operation channel, handed to
/// [`OperationOutput::deliver`](super::OperationOutput::deliver) to send the result.
pub struct Reply {
    channel: Channel,
}

impl Reply {
    pub(crate) fn new(channel: Channel) -> Self {
        Self { channel }
    }

    /// Sends `output` as the result and finishes the channel.
    pub fn finish(self, output: &impl Serialize) -> Result<()> {
        let (channel, body) = self.encode(output)?;
        channel.finish(Some(body))?;
        Ok(())
    }

    /// Sends `output` as the result. The returned channel carries the rest of the output.
    pub fn send(self, output: &impl Serialize) -> Result<Channel> {
        let (channel, body) = self.encode(output)?;
        channel.send(body)?;
        Ok(channel)
    }

    /// Sends `error` as the result and finishes the channel.
    pub(crate) fn fail(self, error: &anyhow::Error) -> Result<()> {
        self.channel.finish(Some(Response::Error(error.into()).encode()))?;
        Ok(())
    }

    /// Encodes the result. An output that cannot be encoded is reported to the caller instead.
    fn encode(self, output: &impl Serialize) -> Result<(Channel, Value)> {
        match serde_json::to_value(output) {
            Ok(value) => Ok((self.channel, Response::Ok(value).encode())),
            Err(error) => {
                let error = anyhow::Error::new(error).context("failed to encode operation output");
                self.fail(&error)?;
                Err(error)
            }
        }
    }
}
