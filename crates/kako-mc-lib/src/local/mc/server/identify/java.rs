//! Builds immutable identity evidence using the shared Java command grammar.

use super::tokens::Syntax;
use crate::local::{
    java::{command, jar},
    mc::server::ServerLaunch,
};
use anyhow::{Context, Result};
use std::{collections::HashMap, path::PathBuf};

pub(super) fn parse(
    directory: PathBuf,
    script: PathBuf,
    line: usize,
    syntax: Syntax,
    executable: String,
    raw: Vec<String>,
    environment: HashMap<String, String>,
) -> Result<ServerLaunch> {
    let windows = syntax == Syntax::Batch;
    let (command, argument_files) = command::expand(raw, &directory, windows)?;
    let mut launch = ServerLaunch {
        executable,
        script,
        line,
        directory,
        command,
        environment,
        argument_files,
        jar_manifest: HashMap::new(),
        path_separator: if windows { ';' } else { ':' },
    };
    if let Some(path) = launch.jar() {
        launch.jar_manifest = jar::manifest(&path)?;
        launch
            .jar_manifest
            .get("main-class")
            .with_context(|| format!("launch JAR {path:?} has no Main-Class"))?;
    }
    Ok(launch)
}
