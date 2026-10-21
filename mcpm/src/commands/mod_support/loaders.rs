//! CLI loader selection and stable ordering of identified metadata.

use std::collections::HashMap;

use clap::{Args, ValueEnum};
use kako_mods_lib::local::{
    mcmod::ModFile,
    modloader::{BUILTIN_LOADERS, LoaderId, ModLoaderMeta},
};

#[derive(Args)]
pub(in crate::commands) struct LoaderArgs {
    /// Select one loader; repeat to select multiple loaders.
    #[arg(long = "loader", value_enum, value_name = "LOADER")]
    loader: Vec<Loader>,
    /// Select comma-separated loaders; combined with all --loader options.
    #[arg(
        long = "loaders",
        value_enum,
        value_delimiter = ',',
        value_name = "LOADERS"
    )]
    loaders: Vec<Loader>,
}

#[derive(Clone, Copy, ValueEnum)]
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

/// Keeps console output stable independently of hash map iteration order.
pub(in crate::commands) fn sort_metadata(file: &mut ModFile) {
    file.metadata.sort_by(|left, right| {
        display_order(left.loader.id())
            .cmp(&display_order(right.loader.id()))
            .then_with(|| left.loader.id().cmp(right.loader.id()))
    });
}

fn display_order(id: &LoaderId) -> usize {
    match id.as_str() {
        "forge" => 0,
        "neoforge" => 1,
        "fabric" => 2,
        _ => 4,
    }
}
