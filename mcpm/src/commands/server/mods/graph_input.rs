//! Resolves server graph inputs and automatic versioned external components.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use kako_mods_lib::local::mcmod::dependency::graph::{
    candidate::ExternalCandidate, identifier::DependencyIdentifier,
};

use super::super::mod_source::ServerMods;

/// Registered server input shared by graph building and dependency checks.
#[derive(Args, Debug)]
pub(super) struct ServerArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(long = "server", value_name = "PATH", default_value = ".")]
    path: PathBuf,
}

impl ServerArgs {
    pub(super) fn resolve(self) -> Result<(ServerMods, [ExternalCandidate; 2])> {
        let source = ServerMods::load(self.path)?;
        let loader = source.container().loader();
        // Dependency ids follow each loader's metadata conventions.
        let loader_modid = match loader.loader.id().as_str() {
            "fabric" => "fabricloader",
            id => id,
        };
        let external = [
            ("minecraft", source.minecraft_version()),
            (loader_modid, loader.version.as_str()),
        ]
        .map(|(id, version)| ExternalCandidate {
            identifier: DependencyIdentifier::from_parts(loader.loader.id().clone(), id.to_owned()),
            version: Some(version.to_owned()),
        });
        Ok((source, external))
    }
}
