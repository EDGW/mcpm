//! Parsed server launch evidence and queries independent of script parsing.

use std::{collections::HashMap, path::PathBuf};

use thiserror::Error;

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
        for (index, arg) in self.game_arguments.iter().enumerate() {
            let value = if arg == name {
                Some(
                    self.game_arguments
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
