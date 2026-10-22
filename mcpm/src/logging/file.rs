//! Open a separate log file under the user's home, named using the local process start time.

use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};

pub(super) fn open(started: SystemTime) -> Result<(File, PathBuf)> {
    let home = dirs::home_dir().context("cannot locate home directory for mcpm logs")?;
    create(&home.join(".mcpm/logs"), started.into())
}

fn create(root: &Path, started: DateTime<Local>) -> Result<(File, PathBuf)> {
    let directory = root.join(started.format("%Y-%m-%d").to_string());
    fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create log directory {}", directory.display()))?;
    // Nanoseconds distinguish rapid starts; hyphens are valid in Windows filenames.
    let time = started.format("%H-%M-%S%.9f");
    let mut path = directory.join(format!("{time}.log"));
    loop {
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // Concurrent processes can observe the same clock tick.
                path = directory.join(format!("{time}-{}.log", uuid::Uuid::new_v4()));
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to create log file {}", path.display()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_uses_start_time_and_existing_logs_are_never_truncated() {
        let root = tempfile::tempdir().unwrap();
        let started = DateTime::parse_from_rfc3339("2026-10-22T23:59:59.123456789+08:00")
            .unwrap()
            .naive_local()
            .and_local_timezone(Local)
            .single()
            .unwrap();
        let (_, path) = create(root.path(), started).unwrap();
        assert_eq!(path, root.path().join("2026-10-22/23-59-59.123456789.log"));
        fs::write(&path, "existing log").unwrap();
        let (_, second) = create(root.path(), started).unwrap();
        assert_ne!(path, second);
        assert_eq!(fs::read_to_string(path).unwrap(), "existing log");
    }
}
