//! Server path and ordered CLI selections, without reading server metadata on the frontend.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::selection::{Selection, SelectionArgs, StdinArgs};

/// Input selection for server mod information and dependency inspection.
#[derive(Args, Debug)]
pub(super) struct FileArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(long = "server", value_name = "PATH", default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    files: SelectionArgs,
    #[command(flatten)]
    stdin: StdinArgs,
}

impl FileArgs {
    /// Appends stdin queries using the requested interpretation.
    pub(super) fn with_stdin(mut self) -> Result<Self> {
        self.files = self.stdin.read(self.files)?;
        Ok(self)
    }

    pub(super) fn into_parts(self) -> (PathBuf, Selection) {
        (self.path, self.files.into_selection())
    }
}
