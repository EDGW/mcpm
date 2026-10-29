//! Literal line-based redirected stdin input shared by commands.

use std::{
    io::{self, BufRead, IsTerminal},
    path::PathBuf,
};

use anyhow::{Context, Result};

/// Appends redirected stdin after explicit arguments. Each nonblank line is one
/// literal path: spaces are preserved and shell quoting is not interpreted.
/// Terminal stdin is left untouched so normal interactive commands do not wait.
pub(in crate::commands) fn append(mut paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    paths.extend(lines()?.into_iter().map(PathBuf::from));
    Ok(paths)
}

/// Reads redirected stdin without interpreting values; blank lines are omitted.
/// Terminal stdin is left untouched.
pub(in crate::commands) fn lines() -> Result<Vec<String>> {
    let mut lines = Vec::new();
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        for line in stdin.lock().lines() {
            let line = line.context("failed to read line from stdin")?;
            if !line.trim().is_empty() {
                lines.push(line);
            }
        }
    }
    Ok(lines)
}
