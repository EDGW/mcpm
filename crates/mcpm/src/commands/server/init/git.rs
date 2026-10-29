//! Git repository discovery and initialization on the host owning an initialized workspace.

use std::{
    fs, io,
    path::Path,
    process::{Command, ExitStatus, Output, Stdio},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Reuses discovered repositories unless a repository in the target itself is required.
pub(super) fn ensure_repository(root: &Path, local: bool) -> Result<Outcome> {
    let outcome = |status| Outcome {
        status,
        path: root.display().to_string(),
    };
    let local_metadata = match fs::symlink_metadata(root.join(".git")) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error).context("failed to inspect target Git metadata"),
    };
    if local && !local_metadata {
        return initialize(root);
    }
    let probe = run(root, &["rev-parse", "--git-dir"])?;
    if probe.status.success() {
        return Ok(outcome(Status::Existing));
    }
    // A failed discovery with existing metadata is an error, not an invitation
    // to reinitialize a damaged or inaccessible repository.
    let absolute = fs::canonicalize(root)
        .with_context(|| format!("failed to resolve Git workspace {root:?}"))?;
    for ancestor in absolute.ancestors() {
        let path = ancestor.join(".git");
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                return Err(GitFailure::new("rev-parse", probe))
                    .with_context(|| format!("failed to discover Git repository at {root:?}"));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect Git metadata {path:?}"));
            }
        }
    }
    initialize(root)
}

/// Git setup result, transported to the frontend for Console rendering.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Outcome {
    pub status: Status,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Created,
    Existing,
}

/// Remote requests must enforce the same conflict as clap before any writes.
pub(super) fn validate_options(local: bool, disabled: bool) -> Result<()> {
    ensure!(
        !(local && disabled),
        "--git and --no-git cannot be used together"
    );
    Ok(())
}

fn initialize(root: &Path) -> Result<Outcome> {
    // Explicitly name the current directory so Git initializes here even inside a parent repository.
    let output = run(root, &["init", "--quiet", "."])?;
    if !output.status.success() {
        return Err(GitFailure::new("init", output))
            .with_context(|| format!("failed to initialize Git repository at {root:?}"));
    }
    tracing::info!(path = %root.display(), "Git repository initialized");
    Ok(Outcome {
        status: Status::Created,
        path: root.display().to_string(),
    })
}

#[derive(Debug, Error)]
#[error("git {operation} failed with {status}: {stderr}")]
struct GitFailure {
    operation: &'static str,
    status: ExitStatus,
    stderr: String,
}

impl GitFailure {
    fn new(operation: &'static str, output: Output) -> Self {
        Self {
            operation,
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        }
    }
}

/// Invokes Git directly and captures diagnostics without inheriting repository routing overrides.
fn run(root: &Path, arguments: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(root)
        .args(arguments)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .stdin(Stdio::null())
        .output()
        .with_context(|| {
            format!("failed to execute Git in {root:?}; use --no-git to skip Git setup")
        })
}
