//! Client manifest library declarations.

use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::LibraryRule;

/// Complete library declaration. Unknown launcher extensions are retained in `extra`.
#[derive(Debug, Clone, Deserialize)]
pub struct MinecraftLibrary {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub url: Option<String>,
    #[serde(default)]
    pub natives: BTreeMap<String, String>,
    #[serde(default)]
    pub rules: Vec<LibraryRule>,
    pub extract: Option<LibraryExtract>,
    #[serde(default)]
    pub checksums: Vec<String>,
    pub clientreq: Option<bool>,
    pub serverreq: Option<bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl MinecraftLibrary {
    pub fn coordinate(&self) -> Result<MavenCoordinate, LibraryCoordinateError> {
        self.name.parse()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadInfo>,
    #[serde(default)]
    pub classifiers: BTreeMap<String, DownloadInfo>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DownloadInfo {
    pub path: Option<String>,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibraryExtract {
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A parsed Maven coordinate, including an optional classifier and extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenCoordinate {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl FromStr for MavenCoordinate {
    type Err = LibraryCoordinateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || LibraryCoordinateError {
            coordinate: value.to_owned(),
        };
        let (coordinate, extension) = value.split_once('@').unwrap_or((value, "jar"));
        let parts: Vec<_> = coordinate.split(':').collect();
        let valid = |part: &str| {
            !part.is_empty()
                && !matches!(part, "." | "..")
                && !part.contains(['/', '\\', ':', '@', '\0'])
        };
        if !(3..=4).contains(&parts.len())
            || !parts.iter().all(|part| valid(part))
            || !valid(extension)
            || parts[0].split('.').any(|part| !valid(part))
        {
            return Err(invalid());
        }
        Ok(Self {
            group: parts[0].into(),
            artifact: parts[1].into(),
            version: parts[2].into(),
            classifier: parts.get(3).map(|value| (*value).into()),
            extension: extension.into(),
        })
    }
}

impl MavenCoordinate {
    /// Relative repository path; validates public fields on every call.
    pub fn path(&self) -> Result<PathBuf, LibraryCoordinateError> {
        let mut coordinate = format!("{}:{}:{}", self.group, self.artifact, self.version);
        if let Some(classifier) = &self.classifier {
            coordinate.push(':');
            coordinate.push_str(classifier);
        }
        coordinate.push('@');
        coordinate.push_str(&self.extension);
        let parsed: Self = coordinate.parse()?;
        if parsed != *self {
            return Err(LibraryCoordinateError { coordinate });
        }
        let classifier = parsed
            .classifier
            .map(|value| format!("-{value}"))
            .unwrap_or_default();
        Ok(PathBuf::from(parsed.group.replace('.', "/"))
            .join(&parsed.artifact)
            .join(&parsed.version)
            .join(format!(
                "{}-{}{classifier}.{}",
                parsed.artifact, parsed.version, parsed.extension
            )))
    }

    pub(crate) fn key(&self) -> (String, String, Option<String>, String) {
        (
            self.group.clone(),
            self.artifact.clone(),
            self.classifier.clone(),
            self.extension.clone(),
        )
    }
}

#[derive(Debug, Error)]
#[error("invalid Maven library coordinate {coordinate:?}")]
pub struct LibraryCoordinateError {
    pub coordinate: String,
}
