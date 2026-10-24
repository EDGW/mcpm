//! CLI selectors for graph roots and optionally versioned external components.

use std::str::FromStr;

use kako_mods_lib::local::{
    mcmod::dependency::graph::identifier::DependencyIdentifier, modloader::LoaderId,
};
use thiserror::Error;

#[derive(Debug, Clone)]
pub(super) struct ModSelector(pub(super) DependencyIdentifier);

#[derive(Debug, Error)]
#[error("invalid mod selector {value:?}: expected a nonempty loader:modid")]
pub(super) struct InvalidModSelector {
    value: String,
}

impl FromStr for ModSelector {
    type Err = InvalidModSelector;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidModSelector {
            value: value.into(),
        };
        let (loader, modid) = value.split_once(':').ok_or_else(invalid)?;
        if loader.is_empty() || modid.is_empty() {
            return Err(invalid());
        }
        Ok(Self(DependencyIdentifier::from_parts(
            LoaderId::new(loader),
            modid.into(),
        )))
    }
}

#[derive(Debug, Clone)]
pub(super) struct ExternalSelector {
    pub identifier: DependencyIdentifier,
    pub version: Option<String>,
}

impl FromStr for ExternalSelector {
    type Err = InvalidExternalSelector;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidExternalSelector(value.to_owned());
        let mut parts = value.splitn(3, ':');
        let loader = parts.next().ok_or_else(invalid)?;
        let id = parts.next().ok_or_else(invalid)?;
        let version = parts.next();
        if loader.is_empty() || id.is_empty() || version.is_some_and(str::is_empty) {
            return Err(invalid());
        }
        Ok(Self {
            identifier: DependencyIdentifier::from_parts(LoaderId::new(loader), id.to_owned()),
            version: version.map(str::to_owned),
        })
    }
}

#[derive(Debug, Error)]
#[error("invalid external selector {0:?}: expected nonempty loader:modid[:version]")]
pub(super) struct InvalidExternalSelector(String);
