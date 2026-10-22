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
