//! Resolves file selection arguments against the registered server mod source.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::mod_support::selection::{Selection, SelectionArgs, StdinArgs};

use super::super::mod_source::ServerMods;

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

    pub(super) fn resolve(self) -> Result<Option<(ServerMods, Selection)>> {
        if self.files.is_empty() {
            crate::commands::mod_support::inspect::prompt_for_input()?;
            return Ok(None);
        }
        let source = ServerMods::load(self.path)?;
        Ok(Some((source, self.files.into_selection())))
    }
}
