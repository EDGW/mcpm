//! Session envelopes and global statuses; command payloads remain opaque to remote dispatch.

use std::io::{BufRead, Write};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use uuid::Uuid;

pub(super) const VERSION: u32 = 4;

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub guid: Uuid,
    pub command: String,
    pub contents: Value,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(super) struct Cancel {
    pub cancel: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Message {
    Logs(Logs),
    Request(Request),
    Cancel(Cancel),
}

/// Built-in log subscription, separate from registered business commands.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Logs {
    pub logs: LogRequest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LogRequest {
    pub guid: Uuid,
    pub level: crate::logging::LogLevel,
}

/// Remote reply lifecycle status, serialized as a lowercase string on the wire.
///
/// `Closed` uses the nil GUID and applies to the connection. `Ready` acknowledges
/// either the connection or a dedicated log subscription. `Data` and
/// `Finished` use an invocation or log subscription GUID. `Error` can apply to either scope.
/// Remote interprets control statuses and global errors; each command interprets
/// its own `Data` payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    /// The connection or dedicated logging session is ready.
    ///
    /// Uses the nil GUID. Contents contain the protocol version, registered string
    /// command IDs and concurrency limit. Clients must validate these before sending requests.
    /// A non-nil GUID acknowledges a log subscription with null contents.
    Ready,
    /// A command-defined message or structured log record that does not end its session.
    ///
    /// Uses the invocation or log subscription GUID. Contents can represent progress, results or business
    /// errors. Messages remain ordered within an invocation but may interleave across invocations.
    Data,
    /// The invocation handler returned normally; no further replies follow for this invocation.
    ///
    /// Uses the invocation GUID with null contents. This indicates normal completion,
    /// not necessarily business success: commands express business outcomes in `Data`.
    /// For a log subscription this ends forwarding before the connection sends `Closed`.
    Finished,
    /// A global error decoded by Remote, with [`GlobalError`] code/message contents.
    ///
    /// A non-nil GUID ends only the target invocation, for example due to an unsupported
    /// command, overload, cancellation or handler failure. Other invocations may continue;
    /// no `Finished` follows for the failed invocation.
    /// The nil GUID signals a connection failure, such as invalid protocol input;
    /// all unfinished invocations fail with the connection.
    Error,
    /// The connection closed normally, using the nil GUID with null contents.
    ///
    /// After the caller closes stdin, the server drains accepted invocations, sends
    /// this status and exits. Earlier invocation failures do not prevent normal
    /// connection shutdown. No further replies may follow this message.
    Closed,
}

impl Status {
    pub(super) fn is_terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Error)
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reply {
    pub guid: Uuid,
    pub status: Status,
    pub(super) contents: Value,
}

impl Reply {
    pub(crate) fn new(guid: Uuid, status: Status, contents: impl Serialize) -> Result<Self> {
        Ok(Self {
            guid,
            status,
            contents: serde_json::to_value(contents)?,
        })
    }

    /// The caller selects its own payload type; remote dispatch never interprets business fields.
    pub(crate) fn decode<T: DeserializeOwned>(self) -> Result<T> {
        serde_json::from_value(self.contents).context("invalid remote reply contents")
    }

    pub(super) fn error(guid: Uuid, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            guid,
            status: Status::Error,
            contents: serde_json::json!({"code":code,"message":message.into()}),
        }
    }

    pub(super) fn failure(guid: Uuid, code: ErrorCode, error: &anyhow::Error) -> Self {
        let error = crate::diagnostics::ErrorReport::capture(error);
        Self {
            guid,
            status: Status::Error,
            contents: serde_json::json!({"code":code,"message":error.message,"causes":error.causes}),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub(super) struct Ready {
    pub version: u32,
    pub commands: Vec<String>,
    pub max_concurrency: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorCode {
    UnsupportedCommand,
    Busy,
    Cancelled,
    HandlerFailed,
    Protocol,
}

#[derive(Debug, Deserialize, thiserror::Error)]
#[error("remote error ({code:?}): {message}")]
pub(crate) struct GlobalError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default)]
    causes: Vec<String>,
}

impl GlobalError {
    pub(super) fn into_remote(mut self) -> anyhow::Error {
        let causes = std::mem::take(&mut self.causes);
        let mut causes = causes.into_iter();
        let error = if let Some(message) = causes.next() {
            crate::diagnostics::ErrorReport {
                message,
                causes: causes.collect(),
            }
            .into_error()
            .context(self)
        } else {
            self.into()
        };
        crate::diagnostics::RemoteError::wrap(error)
    }
}

pub(super) fn write(writer: &mut (impl Write + ?Sized), value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).context("failed to encode remote message")?;
    writer
        .write_all(b"\n")
        .context("failed to terminate remote message")?;
    writer.flush().context("failed to flush remote message")
}

pub(super) fn read<T: DeserializeOwned>(reader: &mut impl BufRead) -> Result<Option<T>> {
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .context("failed to read remote message")?
        == 0
    {
        return Ok(None);
    }
    ensure!(
        line.ends_with('\n'),
        "remote message is missing its terminating newline"
    );
    serde_json::from_str(&line)
        .context("invalid remote JSON message")
        .map(Some)
}
