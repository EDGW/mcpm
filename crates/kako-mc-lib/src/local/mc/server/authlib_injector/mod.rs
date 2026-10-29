//! authlib-injector agent identity in server arguments and user-supplied JARs.
//!
//! Ownership of the managed JAR directory belongs to `mcpm-backend::data::authlib`;
//! this module only interprets evidence.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::local::java::jar;

/// Resolves a user-owned file without inspecting managed metadata or computing its digest.
pub fn manual(root: &Path, value: &str) -> Result<PathBuf> {
    let path = root.join(value);
    let file = std::fs::File::open(&path)
        .with_context(|| format!("failed to open manual authlib-injector JAR {path:?}"))?;
    ensure!(
        file.metadata()?.is_file(),
        "manual authlib-injector JAR is not a file: {path:?}"
    );
    Ok(path)
}

/// Extracts the Java agent's path and opaque options without interpreting other agents.
pub fn agent(argument: &str) -> Option<(&str, &str)> {
    argument.strip_prefix("-javaagent:")?.split_once('=')
}

/// JAR metadata is identity evidence; missing or unreadable evidence is not a guessed match.
pub fn is_agent(path: &Path) -> bool {
    jar::manifest(path).ok().is_some_and(|manifest| {
        manifest
            .get("premain-class")
            .is_some_and(|value| value == "moe.yushi.authlibinjector.Premain")
    })
}
