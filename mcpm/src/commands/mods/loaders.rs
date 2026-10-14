use anyhow::Result;
use clap::{Args, ValueEnum};
use kako_mods_lib::local::modloader::{BUILTIN_LOADERS, ModLoaderSet};

#[derive(Args)]
pub(super) struct LoaderArgs {
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
enum Loader {
    Forge,
    Neoforge,
    Fabric,
    Quilt,
}

impl Loader {
    fn id(self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::Neoforge => "neoforge",
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
        }
    }
}

impl LoaderArgs {
    pub(super) fn is_filtered(&self) -> bool {
        !self.loader.is_empty() || !self.loaders.is_empty()
    }

    pub(super) fn select(&self) -> Result<ModLoaderSet> {
        let mut set = ModLoaderSet::new();
        for metadata in BUILTIN_LOADERS.iter() {
            if !self.is_filtered()
                || self
                    .loader
                    .iter()
                    .chain(&self.loaders)
                    .any(|loader| loader.id() == metadata.id)
            {
                set.add(metadata.clone())?;
            }
        }
        Ok(set)
    }
}
