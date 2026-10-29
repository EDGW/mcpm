//! Error reporting policy, serializable cause chains and remote error identity.

use std::io;

pub(crate) use kako_mc_lib::error::ErrorReport;

use crate::display::{console::ConsoleWriter, error as display};

/// Marks a failure received from another process while preserving its original sources.
///
/// The marker carries the failure's outermost message so reports and events keep their
/// wording; renderers still recognise the type to label the origin.
#[derive(Debug, thiserror::Error)]
#[error("remote error: {message}")]
pub(crate) struct RemoteError {
    message: String,
    #[source]
    error: anyhow::Error,
}

impl RemoteError {
    pub(crate) fn wrap(error: anyhow::Error) -> anyhow::Error {
        let message = error.to_string();
        Self { message, error }.into()
    }
}

pub(crate) fn error(error: &anyhow::Error) {
    crate::logging::record_error(error);
    if crate::logging::is_serving() {
        return;
    }
    display_error(error);
}

/// Render an already-recorded error while a caller suspends its progress display.
pub(crate) fn display_error(error: &anyhow::Error) {
    let remote = kako_mc_lib::error::contains::<RemoteError>(error.as_ref());
    let mut stderr = io::stderr().lock();
    let _ = display::diagnostic(&mut ConsoleWriter::new(&mut stderr), error, remote);
}

/// Captures remote identity separately while retaining the complete diagnostic tree.
pub(crate) fn capture(error: &anyhow::Error) -> (ErrorReport, bool) {
    let remote = kako_mc_lib::error::contains::<RemoteError>(error.as_ref());
    let report = ErrorReport::capture_with(error.as_ref(), |cause| {
        (!cause.is::<RemoteError>()).then(|| cause.to_string())
    });
    (report, remote)
}
