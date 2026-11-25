//! Java executable lookup and environment preparation.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use thiserror::Error;

/// Resolves a configured Java executable path or looks up a bare command name in `PATH`.
pub fn resolve(
    base_dir: &Path,
    value: &str,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<PathBuf> {
    if Path::new(value).is_absolute() || value.contains('/') || value.contains('\\') {
        let path = base_dir.join(value);
        if !path.exists() {
            return Err(JavaExecutableNotFound::new(value).into());
        }
        ensure!(
            is_executable(&path),
            "configured Java executable does not exist or is not executable: {path:?}"
        );
        return Ok(std::fs::canonicalize(path)?);
    }
    let path = environment_value(environment, "PATH")
        .ok_or_else(|| JavaExecutableNotFound::new(value))
        .context("Java command lookup requires PATH")?;
    for directory in std::env::split_paths(path) {
        let directory = if directory.is_absolute() {
            directory
        } else {
            base_dir.join(directory)
        };
        let candidate = directory.join(value);
        if is_executable(&candidate) {
            return Ok(std::fs::canonicalize(candidate)?);
        }
        #[cfg(windows)]
        if Path::new(value).extension().is_none() {
            let candidate = directory.join(format!("{value}.exe"));
            if is_executable(&candidate) {
                return Ok(std::fs::canonicalize(candidate)?);
            }
        }
    }
    Err(JavaExecutableNotFound::new(value).into())
}

/// Maps a `Command::spawn` I/O failure to [`JavaExecutableNotFound`] when the working
/// directory exists and the executable itself is missing, preserving the `io::Error` source.
pub(crate) fn map_spawn_error(
    source: std::io::Error,
    executable: &Path,
    directory: &Path,
) -> anyhow::Error {
    if source.kind() == std::io::ErrorKind::NotFound && directory.is_dir() {
        JavaExecutableNotFound {
            executable: executable.display().to_string(),
            source: Some(source),
        }
        .into()
    } else {
        source.into()
    }
}

/// Builds an environment map from the current process environment with overrides applied.
/// On Windows, keys are matched ASCII case-insensitively before replacement.
pub fn merge_environment(overrides: &BTreeMap<String, String>) -> BTreeMap<OsString, OsString> {
    let mut environment: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (key, value) in overrides {
        #[cfg(windows)]
        environment.retain(|existing, _| !existing.to_string_lossy().eq_ignore_ascii_case(key));
        environment.insert(key.into(), value.into());
    }
    environment
}

/// Looks up an environment variable, matching ASCII case-insensitively on Windows.
pub fn environment_value<'a>(
    environment: &'a BTreeMap<OsString, OsString>,
    key: &str,
) -> Option<&'a OsStr> {
    #[cfg(windows)]
    return environment
        .iter()
        .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_os_str());
    #[cfg(not(windows))]
    environment.get(OsStr::new(key)).map(OsString::as_os_str)
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    #[cfg(windows)]
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return false;
    }
    true
}

/// The configured or discovered Java executable could not be found.
#[derive(Debug, Error)]
#[error("executable {executable:?} was not found")]
pub struct JavaExecutableNotFound {
    pub executable: String,
    #[source]
    pub source: Option<std::io::Error>,
}

impl JavaExecutableNotFound {
    pub(crate) fn new(executable: impl Into<String>) -> Self {
        Self {
            executable: executable.into(),
            source: None,
        }
    }
}
