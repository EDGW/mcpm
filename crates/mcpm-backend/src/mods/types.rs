//! Types shared by more than one mods operation.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::data::cache::mods::ModsCacheSelection;

/// Where a mod-reading operation gets its files.
///
/// `Server` is resolved on the host into the server's mods directory and declared loader,
/// so tools and server commands share one operation and one round trip.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModsScope {
    Paths {
        /// Directory searched by modid/name queries and `--all`; relative files use it too.
        #[serde(default = "default_directory")]
        directory: PathBuf,
        paths: Vec<PathBuf>,
        #[serde(default)]
        loaders: Vec<String>,
    },
    Server {
        path: PathBuf,
    },
}

fn default_directory() -> PathBuf {
    PathBuf::from(".")
}

/// Which Mod cache an operation may use.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModsCachePolicy {
    /// The scope's own default: the global cache for paths, the server runtime otherwise.
    #[default]
    ScopeDefault,
    /// Read and identify without a Mod cache.
    Disabled,
    /// An explicit cache location supplied by the caller.
    Select { selection: ModsCacheSelection },
}
