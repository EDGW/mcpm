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
