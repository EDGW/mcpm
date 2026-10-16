//! Parsed server launch evidence and queries independent of script parsing.

use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result, ensure};

/// A Java launch parsed without executing the script. Paths are absolute and resolved
/// against the statically known JVM working directory. Application and JVM arguments
/// are separated so application text can never masquerade as JVM options.
#[derive(Debug, Clone)]
pub struct ServerLaunch {
    pub executable: String,
    pub script: PathBuf,
    pub line: usize,
    pub directory: PathBuf,
    pub arguments: Vec<String>,
    pub argument_files: Vec<PathBuf>,
    pub main_class: String,
    pub jar: Option<PathBuf>,
    pub classpath: Vec<PathBuf>,
    pub properties: HashMap<String, String>,
    pub game_arguments: Vec<String>,
    /// Separator used by the script's target Java platform, including offline inspection.
    pub path_separator: char,
}

impl ServerLaunch {
    /// One unambiguous game option, accepting both `--name value` and `--name=value`.
    pub fn option(&self, name: &str) -> Result<Option<&str>> {
        let mut found = None;
        for (index, arg) in self.game_arguments.iter().enumerate() {
            let value = if arg == name {
                Some(
                    self.game_arguments
                        .get(index + 1)
                        .with_context(|| format!("missing game option value for {name}"))?
                        .as_str(),
                )
            } else {
                arg.strip_prefix(&format!("{name}="))
            };
            if let Some(value) = value {
                ensure!(
                    !value.trim().is_empty() && !value.starts_with('-'),
                    "invalid game option value for {name}"
                );
                ensure!(found.is_none(), "duplicate game option {name}");
                found = Some(value);
            }
        }
        Ok(found)
    }

    pub fn resolve(&self, path: &str) -> PathBuf {
        self.directory.join(if self.path_separator == ';' {
            path.replace('\\', "/")
        } else {
            path.to_owned()
        })
    }
}
