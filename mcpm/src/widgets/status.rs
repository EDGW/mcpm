//! Browser notifications retain structured diagnostics until the shared renderer presents them.

use crate::{diagnostics::ErrorReport, theme::Role};

/// A transient browser message or a complete failure, without flattening its cause tree.
#[derive(Debug, Clone)]
pub(crate) enum Status {
    Message(String, Role),
    Error { report: ErrorReport, remote: bool },
}

impl Status {
    pub(crate) fn error(error: anyhow::Error) -> Self {
        crate::logging::record_error(&error);
        let (report, remote) = crate::diagnostics::capture(&error);
        Self::Error { report, remote }
    }

    pub(crate) fn lines(&self) -> Vec<String> {
        match self {
            Self::Message(message, _) => vec![crate::display::style::text(message)],
            Self::Error { report, remote } => crate::display::error::lines(report, *remote),
        }
    }

    pub(crate) fn role(&self) -> Role {
        match self {
            Self::Message(_, role) => *role,
            Self::Error { .. } => Role::Danger,
        }
    }
}

impl From<(String, Role)> for Status {
    fn from((message, role): (String, Role)) -> Self {
        Self::Message(message, role)
    }
}
