//! Host-side lookup and SHA-256 verification for bundled release artifacts staged beside
//! `mcpm` / `mcpmd`.
//!
//! Expected digests come from `scripts/artifacts.toml` via `build.rs` so the lock file
//! remains the single source of truth.

use std::{
    env,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use mcpm_lib::download::{Checksum, ChecksumAlgorithm};

const LOADER_INJECTOR_FILENAME: &str = "mcloader-injector-agent.jar";
const LOADER_INJECTOR_SHA256: &str = env!("MCPM_ARTIFACT_MCLOADER_INJECTOR_AGENT_JAR_SHA256");

/// Locates and verifies the bundled `mcloader-injector-agent.jar` beside the running executable.
///
/// Missing or corrupted artifacts are treated as installation/distribution errors rather than
/// mirror source failures.
pub(crate) fn loader_injector_agent() -> Result<PathBuf> {
    let executable = env::current_exe().context("failed to locate the running executable")?;
    let executable = executable
        .canonicalize()
        .with_context(|| format!("failed to resolve running executable {executable:?}"))?;
    let directory = executable
        .parent()
        .with_context(|| format!("running executable has no parent directory: {executable:?}"))?;

    let candidate = resolve_candidate(directory, LOADER_INJECTOR_FILENAME);
    verify_bundled_file(&candidate, LOADER_INJECTOR_FILENAME, LOADER_INJECTOR_SHA256)?;
    Ok(candidate)
}

fn resolve_candidate(directory: &Path, filename: &str) -> PathBuf {
    let candidate = directory.join(filename);
    #[cfg(any(test, debug_assertions))]
    if !candidate.exists()
        && directory.file_name().is_some_and(|name| name == "deps")
        && let Some(profile_dir) = directory.parent()
    {
        return profile_dir.join(filename);
    }
    candidate
}

fn verify_bundled_file(path: &Path, filename: &str, expected_sha256: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => bail!(
            "bundled {filename} at {path:?} is not a regular file; reinstall mcpm or run 'mcpm-tools debug validate'"
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => bail!(
            "missing bundled {filename} beside executable at {path:?}; reinstall mcpm or run 'mcpm-tools debug validate'"
        ),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect bundled {filename} at {path:?}"));
        }
    }

    let checksum = Checksum::new(ChecksumAlgorithm::Sha256, expected_sha256)
        .with_context(|| format!("invalid compiled SHA-256 pin for {filename}"))?;
    let mut file = File::open(path)
        .with_context(|| format!("failed to open bundled {filename} at {path:?}"))?;
    checksum.verify_reader(&mut file).with_context(|| {
        format!(
            "bundled {filename} at {path:?} failed SHA-256 verification; reinstall mcpm or run 'mcpm-tools debug validate'"
        )
    })
}
