//! Shared client directory and version selection arguments.

use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Debug)]
pub(super) struct Input {
    pub path: PathBuf,
    pub inferred: bool,
}

pub(super) fn resolve(path: Option<PathBuf>) -> Result<Input> {
    if let Some(path) = path {
        return Ok(Input {
            path,
            inferred: false,
        });
    }
    let current = std::env::current_dir().context("failed to determine current directory")?;
    // Ancestors run from the current directory toward the filesystem root,
    // so the first exact component match is the last .minecraft in the path.
    match current
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == ".minecraft"))
    {
        Some(path) => Ok(Input {
            path: path.to_owned(),
            inferred: true,
        }),
        None => Ok(Input {
            path: current,
            inferred: false,
        }),
    }
}
