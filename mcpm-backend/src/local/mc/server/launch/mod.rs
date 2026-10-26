//! Parsed server launch evidence and queries independent of script parsing.

use std::{collections::HashMap, path::PathBuf};

pub use crate::local::java::command::{JavaCommand, JavaEntry};
use crate::local::java::jar;
use thiserror::Error;

mod plan;
pub use plan::{LaunchPlan, prepare, prepare_with_overrides, prepare_with_progress};
mod progress;
pub use progress::{Event, Stage};
mod process;
pub use process::{ForegroundStreams, LaunchExit, StopRequest, execute};

/// A Java launch parsed without executing the script. Paths are absolute and resolved
/// against the statically known JVM working directory. Application and JVM arguments
/// are separated so application text can never masquerade as JVM options.
#[derive(Debug, Clone)]
pub struct ServerLaunch {
    pub executable: String,
    pub script: PathBuf,
    pub line: usize,
    pub directory: PathBuf,
    pub command: JavaCommand,
    /// Explicit exported variables only; inherited environment is not snapshotted.
    pub environment: HashMap<String, String>,
    pub argument_files: Vec<PathBuf>,
    pub(crate) jar_manifest: HashMap<String, String>,
    /// Separator used by the script's target Java platform, including offline inspection.
    pub path_separator: char,
}

impl ServerLaunch {
    /// Entry class evidence; JAR manifest data never changes the configured entry kind.
    pub fn main_class(&self) -> &str {
        match &self.command.entry {
            JavaEntry::Class { main_class } => main_class,
            JavaEntry::Jar { .. } => self
                .jar_manifest
                .get("main-class")
                .map(String::as_str)
                .unwrap_or(""),
            JavaEntry::Module { main_class, .. } => main_class.as_deref().unwrap_or(""),
        }
    }

    pub fn jar(&self) -> Option<PathBuf> {
        match &self.command.entry {
            JavaEntry::Jar { path } => Some(self.resolve(path)),
            _ => None,
        }
    }

    pub fn classpath(&self) -> Vec<PathBuf> {
        if let Some(path) = self.jar() {
            jar::manifest_paths(&path, &self.jar_manifest)
        } else {
            self.paths(&["-cp", "-classpath", "--class-path"])
        }
    }

    pub fn modulepath(&self) -> Vec<PathBuf> {
        self.paths(&["-p", "--module-path"])
    }

    fn paths(&self, names: &[&str]) -> Vec<PathBuf> {
        self.command
            .options()
            .filter(|(name, _)| names.contains(name))
            .last()
            .and_then(|(_, value)| value)
            .map(|value| {
                value
                    .split(self.path_separator)
                    .map(|path| self.resolve(path))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn property(&self, key: &str) -> Option<&str> {
        self.command
            .options()
            .filter_map(|(name, value)| {
                (name.strip_prefix("-D") == Some(key)).then_some(value.unwrap_or(""))
            })
            .last()
    }

    pub fn resolve(&self, path: &str) -> PathBuf {
        self.directory.join(if self.path_separator == ';' {
            path.replace('\\', "/")
        } else {
            path.to_owned()
        })
    }

    /// Extracts one option from `--name value` or `--name=value` declarations.
    /// Repeated identical values are accepted; conflicting values remain ambiguous.
    /// This queries identity evidence without validating the command as a whole.
    ///
    /// # Errors
    /// Returns [`ServerOptionError`] when a declaration has no extractable value
    /// or when repeated declarations contain conflicting values.
    pub fn option(&self, name: &str) -> Result<Option<&str>, ServerOptionError> {
        let mut found: Option<&str> = None;
        for (index, arg) in self.command.application.iter().enumerate() {
            let value = if arg == name {
                Some(
                    self.command
                        .application
                        .get(index + 1)
                        .ok_or_else(|| ServerOptionError::MissingValue {
                            name: name.to_owned(),
                        })?
                        .as_str(),
                )
            } else {
                arg.strip_prefix(&format!("{name}="))
            };
            if let Some(value) = value {
                if value.trim().is_empty() || value.starts_with('-') {
                    return Err(ServerOptionError::UnextractableValue {
                        name: name.to_owned(),
                        value: value.to_owned(),
                    });
                }
                if let Some(previous) = found
                    && previous != value
                {
                    return Err(ServerOptionError::ConflictingValues {
                        name: name.to_owned(),
                        first: previous.to_owned(),
                        second: value.to_owned(),
                    });
                }
                found = Some(value);
            }
        }
        Ok(found)
    }
}

/// Failures to extract one option's identity evidence from a server command.
#[derive(Debug, Error)]
pub enum ServerOptionError {
    /// A separate option declaration has no following argument.
    #[error("missing game option value for {name}")]
    MissingValue { name: String },
    /// The following argument is empty or begins another option.
    #[error("no extractable game option value for {name}: {value:?}")]
    UnextractableValue { name: String, value: String },
    /// Repeated declarations disagree and no precedence rule is assumed.
    #[error("conflicting game option {name}: {first:?} versus {second:?}")]
    ConflictingValues {
        name: String,
        first: String,
        second: String,
    },
}
