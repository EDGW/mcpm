//! Protocol v6: line-delimited JSON frames between a caller and a serve session.
//!
//! One request starts one call. `Ready` names the build and the host operations the serve
//! process provides; every later frame carries the call's GUID, and each call subscribes to
//! its own log records instead of a connection-wide subscription.

use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use uuid::Uuid;

use crate::error::DispatchError;

/// Revision of this module; both ends must agree before any call is sent.
pub const VERSION: u32 = 6;

/// One protocol frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub guid: Uuid,
    pub status: Status,
    contents: Value,
}

impl Frame {
    /// The connection handshake sent by serve; the nil GUID scopes it to the connection.
    pub fn ready(build: impl Into<String>, operations: Vec<String>) -> Self {
        let contents = Ready {
            version: VERSION,
            build: build.into(),
            operations,
        };
        Self {
            guid: Uuid::nil(),
            status: Status::Ready,
            contents: encode(&contents),
        }
    }

    /// A structured log record belonging to one call.
    pub fn log(guid: Uuid, record: LogRecord) -> Self {
        Self {
            guid,
            status: Status::Log,
            contents: encode(&record),
        }
    }

    /// An event emitted by one call; the call continues afterwards.
    pub fn data(guid: Uuid, event: Value) -> Self {
        Self {
            guid,
            status: Status::Data,
            contents: event,
        }
    }

    /// The call returned; the contents carry the operation output.
    pub fn finished(guid: Uuid, output: Value) -> Self {
        Self {
            guid,
            status: Status::Finished,
            contents: output,
        }
    }

    /// The call ended without an output.
    pub fn failed(guid: Uuid, failure: FailurePayload) -> Self {
        Self {
            guid,
            status: Status::Failed,
            contents: encode(&failure),
        }
    }

    /// The connection drained every accepted call and closes normally.
    pub fn closed() -> Self {
        Self {
            guid: Uuid::nil(),
            status: Status::Closed,
            contents: Value::Null,
        }
    }

    pub fn guid(&self) -> Uuid {
        self.guid
    }

    pub fn status(&self) -> Status {
        self.status
    }

    /// Decodes the contents as the frame's declared payload type.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.contents.clone())
    }

    /// Consumes the frame, keeping its payload encoded.
    pub fn into_contents(self) -> Value {
        self.contents
    }
}

/// Frame status; the GUID decides whether it belongs to the connection or to one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Connection handshake with [`Ready`] contents; never repeats during the connection.
    Ready,
    /// A [`LogRecord`] for the call identified by the frame GUID.
    Log,
    /// An event for the call identified by the frame GUID.
    Data,
    /// The call returned; the contents carry its output and no further frame follows.
    Finished,
    /// The call ended without an output; the contents carry a [`FailurePayload`].
    Failed,
    /// The connection closed normally after draining accepted calls; nil GUID.
    Closed,
}

/// Handshake payload: what the serve process is and which host operations it provides.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ready {
    pub version: u32,
    pub build: String,
    pub operations: Vec<String>,
}

/// Message sent to serve: start a call, or cancel one that is already running.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Cancel(Cancel),
}

/// One call request; the input stays encoded until serve resolves the operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub guid: Uuid,
    pub operation: String,
    /// Log selection for this call only.
    pub level: LogLevel,
    pub input: Value,
}

/// Cancels one running call; later frames for this GUID are never read by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cancel {
    pub cancel: Uuid,
}

/// Why a call ended without an output; the tag separates operation from dispatch failures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "failure", rename_all = "snake_case")]
pub enum FailurePayload {
    /// The operation failed; the error is the operation's own encoding.
    Operation { error: Value },
    /// The executor could not route, decode or encode the call.
    Dispatch { error: DispatchError },
    /// The call was cancelled before it produced a result.
    Cancelled,
}

/// Log selection of one call, ordered from least to most verbose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Whether a tracing event at `level` is forwarded for this selection.
    pub fn allows(self, level: &tracing::Level) -> bool {
        let selected = match self {
            Self::Off => return false,
            Self::Error => 1,
            Self::Warn => 2,
            Self::Info => 3,
            Self::Debug => 4,
            Self::Trace => 5,
        };
        severity(level) <= selected
    }
}

fn severity(level: &tracing::Level) -> u8 {
    match *level {
        tracing::Level::ERROR => 1,
        tracing::Level::WARN => 2,
        tracing::Level::INFO => 3,
        tracing::Level::DEBUG => 4,
        tracing::Level::TRACE => 5,
    }
}

/// Structured log record; `fields` and `spans` keep their original string values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogRecord {
    level: LogLevel,
    target: String,
    fields: BTreeMap<String, String>,
    spans: Vec<LogSpan>,
}

impl LogRecord {
    pub(crate) fn new(
        level: LogLevel,
        target: &str,
        fields: BTreeMap<String, String>,
        spans: Vec<LogSpan>,
    ) -> Self {
        Self {
            level,
            target: target.to_owned(),
            fields,
            spans,
        }
    }

    pub fn level(&self) -> LogLevel {
        self.level
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn fields(&self) -> &BTreeMap<String, String> {
        &self.fields
    }

    pub fn spans(&self) -> &[LogSpan] {
        &self.spans
    }

    /// Removes the `message` field, which replay renders separately from the other fields.
    pub fn take_message(&mut self) -> String {
        self.fields.remove("message").unwrap_or_default()
    }
}

/// One span of a record's scope, root first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogSpan {
    name: String,
    fields: BTreeMap<String, String>,
}

impl LogSpan {
    pub(crate) fn new(name: &str, fields: BTreeMap<String, String>) -> Self {
        Self {
            name: name.to_owned(),
            fields,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fields(&self) -> &BTreeMap<String, String> {
        &self.fields
    }
}

pub(crate) fn write(writer: &mut (impl Write + ?Sized), value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, value).map_err(invalid_data)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

pub(crate) fn read<T: DeserializeOwned>(reader: &mut impl BufRead) -> io::Result<Option<T>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if !line.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "protocol message is missing its terminating newline",
        ));
    }
    serde_json::from_str(&line).map(Some).map_err(invalid_data)
}

fn encode(value: &impl Serialize) -> Value {
    // Every payload built here is plain JSON: object keys are strings and no float is NaN.
    serde_json::to_value(value).expect("protocol payload must be representable as JSON")
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::*;

    fn round_trip<T>(value: &impl Serialize) -> T
    where
        T: DeserializeOwned,
    {
        let mut bytes = Vec::new();
        write(&mut bytes, value).unwrap();
        read(&mut Cursor::new(bytes)).unwrap().unwrap()
    }

    #[test]
    fn frames_round_trip_with_their_payloads() {
        let guid = Uuid::from_u128(7);
        let record = LogRecord::new(
            LogLevel::Info,
            "mcpm",
            BTreeMap::from([("message".to_owned(), "hello".to_owned())]),
            vec![LogSpan::new("operation", BTreeMap::new())],
        );
        for frame in [
            Frame::ready("build", vec!["mods.jar.read".to_owned()]),
            Frame::log(guid, record),
            Frame::data(guid, json!({"event":"progress"})),
            Frame::finished(guid, json!(7)),
            Frame::failed(guid, FailurePayload::Cancelled),
            Frame::closed(),
        ] {
            assert_eq!(round_trip::<Frame>(&frame), frame);
        }
        let ready: Ready = Frame::ready("build", vec!["a".to_owned()]).decode().unwrap();
        assert_eq!(
            ready,
            Ready {
                version: VERSION,
                build: "build".to_owned(),
                operations: vec!["a".to_owned()],
            }
        );
    }

    #[test]
    fn failure_payloads_keep_operation_and_dispatch_errors_apart() {
        let operation = Frame::failed(
            Uuid::from_u128(1),
            FailurePayload::Operation {
                error: json!({"kind":"AlreadyRunning"}),
            },
        );
        let dispatch = Frame::failed(
            Uuid::from_u128(2),
            FailurePayload::Dispatch {
                error: DispatchError::UnknownOperation {
                    operation: "mods.jar.read".to_owned(),
                },
            },
        );
        assert_eq!(
            round_trip::<Frame>(&operation).decode::<FailurePayload>().unwrap(),
            FailurePayload::Operation {
                error: json!({"kind":"AlreadyRunning"}),
            }
        );
        assert_eq!(
            round_trip::<Frame>(&dispatch).decode::<FailurePayload>().unwrap(),
            FailurePayload::Dispatch {
                error: DispatchError::UnknownOperation {
                    operation: "mods.jar.read".to_owned(),
                },
            }
        );
    }

    #[test]
    fn messages_distinguish_requests_from_cancellations() {
        let request = Message::Request(Request {
            guid: Uuid::from_u128(3),
            operation: "mods.select".to_owned(),
            level: LogLevel::Debug,
            input: json!({"paths":["a.jar"]}),
        });
        assert_eq!(round_trip::<Message>(&request), request);
        let cancel = Message::Cancel(Cancel {
            cancel: Uuid::from_u128(3),
        });
        assert_eq!(round_trip::<Message>(&cancel), cancel);
    }

    #[test]
    fn malformed_frames_are_rejected_instead_of_guessed() {
        assert!(read::<Frame>(&mut Cursor::new(b"".as_slice())).unwrap().is_none());
        for line in [
            "{\"guid\":\"00000000-0000-0000-0000-000000000000\",\"status\":\"ready\"}\n",
            "{\"guid\":\"00000000-0000-0000-0000-000000000000\",\"status\":\"closed\",\"contents\":null,\"extra\":1}\n",
            "{\"guid\":\"00000000-0000-0000-0000-000000000000\",\"status\":\"unknown\",\"contents\":null}\n",
        ] {
            let read = read::<Frame>(&mut Cursor::new(line.as_bytes()));
            assert!(read.is_err(), "{line}");
        }
        assert!(
            read::<Frame>(&mut Cursor::new(&b"{\"guid\":"[..]))
                .is_err()
        );
    }

    #[test]
    fn log_selection_filters_by_severity() {
        assert!(LogLevel::Info.allows(&tracing::Level::WARN));
        assert!(LogLevel::Info.allows(&tracing::Level::INFO));
        assert!(!LogLevel::Info.allows(&tracing::Level::DEBUG));
        assert!(!LogLevel::Off.allows(&tracing::Level::ERROR));
    }
}
