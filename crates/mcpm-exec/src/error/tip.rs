//! Advice attached to errors, kept apart from the failure cause chain.
//!
//! A Tip is a [`Rich`] value built with [`mcpm_rich::rich!`]: semantic text without styling.
//! [`TippedError`] sits in the `source()` chain only so that capture can find it; it contributes
//! no message of its own, and frontends render the Tip separately from `caused by` lines.

use std::{error::Error as StdError, fmt};

use mcpm_rich::{Rich, rich};
use serde::{Deserialize, Serialize};

/// Attaches local and remote Tips to an error without adding a failure message.
///
/// `local` is shown when the error is observed in the process that created it; `remote` is shown
/// once the error has crossed a [`RemoteError`](super::RemoteError) boundary. Either or both may
/// be absent.
#[derive(Debug)]
pub struct TippedError {
    local: Option<Rich>,
    remote: Option<Rich>,
    source: anyhow::Error,
}

impl TippedError {
    /// Separate local and remote Tips; either may be `None`.
    pub fn new(
        source: impl Into<anyhow::Error>,
        local: Option<Rich>,
        remote: Option<Rich>,
    ) -> Self {
        Self {
            local,
            remote,
            source: source.into(),
        }
    }

    /// An executable lookup failed where this error was created.
    ///
    /// The non-interactive SSH/PATH advice is shown only after the error crosses a
    /// [`RemoteError`](super::RemoteError) boundary.
    pub fn executable_not_found(executable: impl Into<String>) -> Self {
        let executable = executable.into();
        Self::new(
            anyhow::anyhow!("executable {executable:?} was not found"),
            None,
            Some(ssh_executable_tip()),
        )
    }

    pub fn local(&self) -> Option<&Rich> {
        self.local.as_ref()
    }

    pub fn remote(&self) -> Option<&Rich> {
        self.remote.as_ref()
    }
}

impl fmt::Display for TippedError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, output)
    }
}

impl StdError for TippedError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

fn ssh_executable_tip() -> Rich {
    rich!(
        "
        If you are connecting to a Unix-like system, mcpm uses non-interactive SSH;
        shell startup files such as `.bashrc` and `.zshrc` may NOT be read.
        We recommend using absolute executable paths.

        > To find executables through PATH instead:

        > - zsh: set PATH in `~/.zshenv`.
        > - bash or other shells: consider SSH User Environment (`~/.ssh/environment`),
        >   or the SSH client's SetEnv/SendEnv, if the server permits them through
        >   PermitUserEnvironment/AcceptEnv.
        "
    )
}

/// Attaches Tips to the error of a `Result`.
pub trait TipExt<T> {
    /// The same Tip in both scopes.
    fn tip(self, tip: Rich) -> anyhow::Result<T>;
    /// Separate local and remote Tips; either may be `None`.
    fn tips(self, local: Option<Rich>, remote: Option<Rich>) -> anyhow::Result<T>;
}

impl<T, E: Into<anyhow::Error>> TipExt<T> for Result<T, E> {
    fn tip(self, tip: Rich) -> anyhow::Result<T> {
        self.tips(Some(tip.clone()), Some(tip))
    }

    fn tips(self, local: Option<Rich>, remote: Option<Rich>) -> anyhow::Result<T> {
        self.map_err(|error| TippedError::new(error, local, remote).into())
    }
}

/// Tips of one [`TippedError`] node as captured into an [`ErrorReport`](super::ErrorReport).
///
/// `local` is what the holder of the report shows. A node captured after crossing a
/// [`RemoteError`](super::RemoteError) stores its remote Tip in both fields, so the choice is
/// final however often the report is forwarded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportTip {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<Rich>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<Rich>,
}
