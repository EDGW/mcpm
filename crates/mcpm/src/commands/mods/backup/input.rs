//! Shared directory inputs from positional arguments and literal stdin lines.

use std::{collections::HashSet, path::PathBuf};

use anyhow::{Result, ensure};

use crate::commands::stdin;

pub(super) fn directories(arguments: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut directories = stdin::append(arguments)?;
    ensure!(
        !directories.is_empty(),
        "provide directories as arguments or one per stdin line"
    );
    let mut seen = HashSet::new();
    directories.retain(|path| seen.insert(path.clone()));
    Ok(directories)
}
