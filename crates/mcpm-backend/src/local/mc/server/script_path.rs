//! Validation of portable server-relative script paths and filesystem containment.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Validates without requiring the script to exist; existing symlinks must stay inside the server.
pub(super) fn validate(root: &Path, script: &Path) -> Result<(), ServerScriptError> {
    let text = script.to_string_lossy();
    if script.has_root() || text.starts_with(['/', '\\']) || text.as_bytes().get(1) == Some(&b':') {
        return Err(ServerScriptError::Absolute {
            script: script.into(),
        });
    }
    let mut depth = 0usize;
    for part in text.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| ServerScriptError::Outside {
                        script: script.into(),
                    })?;
            }
            _ => depth += 1,
        }
    }
    if depth == 0 || text.contains('\0') {
        return Err(ServerScriptError::Invalid {
            script: script.into(),
        });
    }
    let inspect = |path: &Path, source| ServerScriptError::Inspect {
        path: path.into(),
        source,
    };
    let root = fs::canonicalize(root).map_err(|source| inspect(root, source))?;
    let mut resolved = root.clone();
    for part in text
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if part == ".." {
            resolved.pop();
        } else {
            resolved.push(part);
            match fs::canonicalize(&resolved) {
                Ok(path) => resolved = path,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // Missing scripts are valid snapshots; dangling links cannot establish containment.
                    match fs::symlink_metadata(&resolved) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(source) => return Err(inspect(&resolved, source)),
                        Ok(_) => return Err(inspect(&resolved, error)),
                    }
                }
                Err(source) => return Err(inspect(&resolved, source)),
            }
        }
        if !resolved.starts_with(&root) {
            return Err(ServerScriptError::Outside {
                script: script.into(),
            });
        }
    }
    Ok(())
}

/// Invalid script locations or failures to inspect their filesystem boundaries.
#[derive(Debug, Error)]
pub enum ServerScriptError {
    #[error("server start script must be relative to the Server directory: {script:?}")]
    Absolute { script: PathBuf },
    #[error("server start script escapes the Server directory: {script:?}")]
    Outside { script: PathBuf },
    #[error("server start script must name a nonempty relative file path: {script:?}")]
    Invalid { script: PathBuf },
    #[error("failed to resolve server start script path {path:?}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
