//! CLI loader identifiers; loader implementation selection belongs to the Host.

use clap::{Args, ValueEnum};

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
    /// Carry only explicit loader choices; the Host resolves their implementations.
    pub(in crate::commands) fn ids(&self) -> Vec<String> {
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
}
