//! Wire format: one [`Frame`] per line of compact JSON.
//!
//! This module is the only place that encodes or decodes frames.

// Imports

use std::fmt;

use serde::de::{SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

/// One message on the wire.
///
/// `id` is the channel GUID. A frame without a body is only valid when it finishes the channel.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Frame {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Flags::is_empty")]
    pub flags: Flags,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub body: Option<Value>,
}

impl Frame {
    /// A regular message on channel `id`.
    pub fn message(id: Uuid, body: Value) -> Self {
        Self { id, flags: Flags::default(), body: Some(body) }
    }

    /// The last frame of channel `id`, optionally carrying a final message.
    pub fn finish(id: Uuid, body: Option<Value>) -> Self {
        Self { id, flags: Flags { finish: true }, body }
    }

    /// Appends this frame to `line` as compact JSON followed by `\n`.
    pub fn encode(&self, line: &mut Vec<u8>) -> Result<(), serde_json::Error> {
        serde_json::to_writer(&mut *line, self)?;
        line.push(b'\n');
        Ok(())
    }

    /// Decodes one line, with or without its trailing newline.
    pub fn decode(line: &[u8]) -> Result<Self, FrameError> {
        let frame: Self = serde_json::from_slice(line).map_err(FrameError::Json)?;
        if frame.body.is_none() && !frame.flags.finish {
            return Err(FrameError::MissingBody(frame.id));
        }
        Ok(frame)
    }
}

/// Invalid line received from the peer.
#[derive(Debug, Error)]
pub enum FrameError {
    #[error("frame is not valid JSON or has unknown flags")]
    Json(#[source] serde_json::Error),
    #[error("frame on channel {0} has no body and does not finish the channel")]
    MissingBody(Uuid),
}

/// Distinguishes `"body": null` (a null message) from a missing body.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// Frame flags, encoded as an array of flag names. Unknown names are rejected.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Flags {
    /// The channel is unusable after this frame, for both sides.
    pub finish: bool,
}

impl Flags {
    fn is_empty(&self) -> bool {
        !self.finish
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Flag {
    Finish,
}

impl Serialize for Flags {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(usize::from(self.finish)))?;
        if self.finish {
            seq.serialize_element(&Flag::Finish)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Flags {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FlagsVisitor;

        impl<'de> Visitor<'de> for FlagsVisitor {
            type Value = Flags;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an array of flag names")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Flags, A::Error> {
                let mut flags = Flags::default();
                while let Some(flag) = seq.next_element()? {
                    match flag {
                        Flag::Finish => flags.finish = true,
                    }
                }
                Ok(flags)
            }
        }

        deserializer.deserialize_seq(FlagsVisitor)
    }
}
