//! Error reporting policy, serializable cause chains and remote error identity.

use std::io;

pub(crate) use mcpm_backend::error::{ErrorReport, RemoteError};

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
    let remote = mcpm_backend::error::contains::<RemoteError>(error.as_ref());
    let mut stderr = io::stderr().lock();
    let _ = display::diagnostic(&mut ConsoleWriter::new(&mut stderr), error, remote);
}

/// Captures remote identity separately while retaining the complete diagnostic tree.
pub(crate) fn capture(error: &anyhow::Error) -> (ErrorReport, bool) {
    let remote = mcpm_backend::error::contains::<RemoteError>(error.as_ref());
    let report = ErrorReport::capture_with(error.as_ref(), |cause| {
        (!cause.is::<RemoteError>()).then(|| cause.to_string())
    });
    (report, remote)
}
