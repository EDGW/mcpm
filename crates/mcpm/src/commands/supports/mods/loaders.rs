//! CLI loader selection and stable ordering of identified metadata.

use std::collections::HashMap;

use clap::{Args, ValueEnum};
use kako_mc_lib::local::modloader::{BUILTIN_LOADERS, LoaderId, ModLoaderMeta};

#[derive(Args, Debug)]
pub(in crate::commands) struct LoaderArgs {
    /// Select loaders, optionally comma-separated; repeat to select more.
    #[arg(
        short = 'l',
        long = "loader",
        value_enum,
        value_delimiter = ',',
        value_name = "LOADER"
    )]
    loader: Vec<Loader>,
    /// Select comma-separated loaders; combined with all --loader options.
    #[arg(
        short = 'L',
        long = "loaders",
        value_enum,
        value_delimiter = ',',
        value_name = "LOADERS"
    )]
    loaders: Vec<Loader>,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
pub(in crate::commands) enum Loader {
    Forge,
    Neoforge,
    Fabric,
}

impl Loader {
    pub(in crate::commands) fn id(self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::Neoforge => "neoforge",
            Self::Fabric => "fabric",
        }
    }
}

impl LoaderArgs {
    pub(in crate::commands) fn is_filtered(&self) -> bool {
        !self.loader.is_empty() || !self.loaders.is_empty()
    }

    /// Selected loader identifiers, sorted for a stable request.
    pub(in crate::commands) fn ids(&self) -> Vec<String> {
        if !self.is_filtered() {
            return Vec::new();
        }
        let mut ids: Vec<_> = self
            .loader
            .iter()
            .chain(&self.loaders)
            .map(|loader| loader.id().to_owned())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub(in crate::commands) fn select(&self) -> HashMap<LoaderId, ModLoaderMeta> {
        if !self.is_filtered() {
            return BUILTIN_LOADERS.clone();
        }

        self.loader
            .iter()
            .chain(&self.loaders)
            .filter_map(|loader| {
                LoaderId::from(loader.id())
                    .metadata(&BUILTIN_LOADERS)
                    .map(|metadata| (metadata.id().clone(), metadata.clone()))
            })
            .collect()
    }
}
