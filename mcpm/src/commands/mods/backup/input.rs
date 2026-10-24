//! Shared backup directory inputs from positional arguments and literal stdin lines.

use std::{collections::HashSet, path::PathBuf};

use anyhow::{Result, ensure};

use crate::commands::stdin;

pub(super) fn directories(arguments: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut backups = stdin::append(arguments)?;
    ensure!(
        !backups.is_empty(),
        "provide backup directories as arguments or one per stdin line"
    );
    let mut seen = HashSet::new();
    backups.retain(|path| seen.insert(path.clone()));
    Ok(backups)
}
