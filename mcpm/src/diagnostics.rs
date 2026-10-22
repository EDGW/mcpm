//! User-facing error chains rendered with the shared stderr theme policy.

use serde::{Deserialize, Serialize};

use crate::{
    terminal,
    theme::{self, Role},
};

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
    if remote && !crate::logging::remote_logs_enabled() {
        return;
    }
    for (depth, cause) in visible_chain(error).enumerate() {
        let indentation = if depth == 0 { "" } else { "  " };
        let label = if depth > 0 {
            "caused by:"
        } else if remote {
            "remote error:"
        } else {
            "error:"
        };
        if terminal::stderr_color_enabled() {
            eprintln!(
                "{indentation}{} {}",
                theme::console(Role::Danger).style(label),
                theme::console(Role::Normal).style(cause)
            );
        } else {
            eprintln!("{indentation}{label} {cause}");
        }
    }
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
        let mut chain = visible_chain(error).map(ToString::to_string);
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
