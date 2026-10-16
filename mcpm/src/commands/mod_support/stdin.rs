//! Line-based stdin paths shared by mod list, info and dependency commands.

use std::{
    io::{self, BufRead, IsTerminal},
    path::PathBuf,
};

use anyhow::{Context, Result};

/// Appends redirected stdin after explicit arguments. Each nonblank line is one
/// literal path: spaces are preserved and shell quoting is not interpreted.
/// Terminal stdin is left untouched so normal interactive commands do not wait.
pub(in crate::commands) fn append(mut paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        for line in stdin.lock().lines() {
            let line = line.context("failed to read mod path from stdin")?;
            if !line.trim().is_empty() {
                paths.push(PathBuf::from(line));
            }
        }
    }
    Ok(paths)
}
