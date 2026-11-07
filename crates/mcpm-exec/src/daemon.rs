//! Locate the local daemon, keeping installer registration decoding in one place.

use anyhow::{Context, Result, ensure};
use std::{fs, io, path::PathBuf};

/// Prefer the running suite's sibling daemon, then the installer record, then PATH.
pub fn executable() -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to locate the running executable")?;
    let name = if cfg!(windows) { "mcpmd.exe" } else { "mcpmd" };
    let sibling = current.with_file_name(name);
    if sibling.is_file() {
        return Ok(sibling);
    }
    #[cfg(windows)]
    let record =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?)
            .join("mcpm/mcpmd.path");
    #[cfg(not(windows))]
    let record = PathBuf::from(std::env::var_os("HOME").context("HOME is unavailable")?)
        .join(".mcpm/mcpmd.path");
    match fs::read_to_string(&record) {
        Ok(value) => {
            let path =
                decode(&value).with_context(|| format!("invalid daemon record {record:?}"))?;
            ensure!(
                path.is_file(),
                "registered mcpmd is not a file: {path:?} (record: {record:?})"
            );
            Ok(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::warn!(path = ?record, "mcpmd.path not found; trying mcpmd on PATH");
            Ok(PathBuf::from(name))
        }
        Err(error) => {
            Err(error).with_context(|| format!("failed to read daemon record {record:?}"))
        }
    }
}

fn decode(value: &str) -> Result<PathBuf> {
    let path = value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value);
    ensure!(
        !path.is_empty() && !path.contains(['\0', '\r', '\n']),
        "expected one absolute executable path"
    );
    let path = PathBuf::from(path);
    ensure!(path.is_absolute(), "expected an absolute executable path");
    Ok(path)
}
