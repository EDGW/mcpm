//! Error reporting policy, serializable cause chains and remote error identity.

use std::io;

use serde::{Deserialize, Serialize};

use crate::display::{console::ConsoleWriter, error as display};

pub(crate) fn error(error: &anyhow::Error) {
    crate::logging::record_error(error);
    if crate::logging::stream::is_forwarding() {
        return;
    }
    display_error(error);
}

/// Render an already-recorded error while a caller suspends its progress display.
pub(crate) fn display_error(error: &anyhow::Error) {
    let remote = error.chain().any(|cause| cause.is::<RemoteError>());
    let mut stderr = io::stderr().lock();
    let _ = display::diagnostic(&mut ConsoleWriter::new(&mut stderr), error, remote);
}

fn visible_chain(
    error: &anyhow::Error,
) -> impl Iterator<Item = &(dyn std::error::Error + 'static)> {
    error.chain().filter(|cause| !cause.is::<RemoteError>())
}

/// Serializable error contexts; messages are never split on punctuation to recover causes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ErrorReport {
    pub message: String,
    #[serde(default)]
    pub causes: Vec<String>,
}

impl ErrorReport {
    pub(crate) fn capture(error: &anyhow::Error) -> Self {
        Self::capture_with(error, ToString::to_string)
    }

    /// Allows Display to shorten typed causes while retaining every context in order.
    pub(crate) fn capture_with(
        error: &anyhow::Error,
        message: impl FnMut(&(dyn std::error::Error + 'static)) -> String,
    ) -> Self {
        let mut chain = visible_chain(error).map(message);
        Self {
            message: chain.next().unwrap_or_default(),
            causes: chain.collect(),
        }
    }

    pub(crate) fn into_error(self) -> anyhow::Error {
        let mut chain = std::iter::once(self.message).chain(self.causes).rev();
        let mut error =
            anyhow::Error::msg(chain.next().expect("error report contains its message"));
        for context in chain {
            error = error.context(context);
        }
        error
    }
}

/// Marks an error received from another process while preserving its source chain.
#[derive(Debug, thiserror::Error)]
#[error("remote operation failed")]
pub(crate) struct RemoteError(#[source] anyhow::Error);

impl RemoteError {
    pub(crate) fn wrap(error: anyhow::Error) -> anyhow::Error {
        Self(error).into()
    }
}
