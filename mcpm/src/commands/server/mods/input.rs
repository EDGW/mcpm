//! Server path and ordered CLI selections, without reading server metadata on the frontend.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::commands::supports::mods::selection::{ItemArgs, Selection};

/// Input selection for server mod information and dependency inspection.
#[derive(Args, Debug)]
pub(super) struct FileArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(long = "server", value_name = "PATH", default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    items: ItemArgs,
}

impl FileArgs {
    /// Appends stdin queries using the requested interpretation.
    pub(super) fn read(self) -> Result<(PathBuf, Selection)> {
        Ok((self.path, self.items.read()?))
    }
}
