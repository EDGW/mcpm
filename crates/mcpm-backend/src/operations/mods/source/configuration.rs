//! Safe source specifications and their client-free identity interpretation.

use std::{convert::Infallible, fmt, str::FromStr};

use anyhow::{Result, anyhow};
use mcpm_lib::source::curseforge::CurseForgeSource;
use mcpm_lib::source::{SourceId, SummaryRequirements};
use serde::{Deserialize, Serialize};

use super::Credential;

/// An opaque source specification, including any request credentials.
///
/// Collection and deserialization deliberately accept every string. Request
/// validation happens only when a cache miss needs a source client.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Configuration(Credential);

impl Configuration {
    /// A fixed safe label, never a fragment of an unrecognized specification.
    pub fn label(&self) -> &'static str {
        match self.kind() {
            Some(Kind::Modrinth) => "modrinth",
            Some(Kind::CurseForge { .. }) => "curseforge",
            None => "unknown",
        }
    }

    /// Whether no source specification was supplied.
    pub fn is_empty(&self) -> bool {
        self.0.expose().is_empty()
    }

    /// Returns stable identity and digest requirements without building a client.
    ///
    /// An empty direct CurseForge API key is valid metadata; it is rejected only
    /// if a request client must actually be constructed.
    pub fn describe(&self) -> Result<Description> {
        let (label, requirements) = match self.kind() {
            Some(Kind::Modrinth) => ("modrinth", SummaryRequirements::default()),
            Some(Kind::CurseForge { .. }) => (
                CurseForgeSource::IDENTIFICATION_CACHE_ID,
                CurseForgeSource::SUMMARY_REQUIREMENTS,
            ),
            None => {
                return Err(anyhow!(
                    "source must be modrinth, curseforge, or curseforge@KEY"
                ));
            }
        };
        Ok(Description {
            id: SourceId(label.into()),
            cache_id: SourceId(label.into()),
            requirements,
        })
    }

    /// The single interpretation boundary shared by metadata and client creation.
    pub(super) fn kind(&self) -> Option<Kind<'_>> {
        match self.0.expose() {
            "modrinth" => Some(Kind::Modrinth),
            "curseforge" => Some(Kind::CurseForge { key: None }),
            value => value
                .strip_prefix("curseforge@")
                .map(|key| Kind::CurseForge { key: Some(key) }),
        }
    }
}

impl From<String> for Configuration {
    fn from(value: String) -> Self {
        Self(Credential::new(value))
    }
}

impl From<&str> for Configuration {
    fn from(value: &str) -> Self {
        Self(Credential::new(value))
    }
}

impl FromStr for Configuration {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(value))
    }
}

impl fmt::Debug for Configuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Configuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Source identity and digest requirements, independent of request credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Description {
    pub id: SourceId,
    pub cache_id: SourceId,
    pub requirements: SummaryRequirements,
}

pub(super) enum Kind<'a> {
    Modrinth,
    CurseForge { key: Option<&'a str> },
}
