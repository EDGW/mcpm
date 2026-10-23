//! Shared explicit cache-write CLI policy and its transportable file-host options.

use super::Options;
use crate::commands::{cache_support::FileStrategy, execution::default_parallelism};
use anyhow::Result;
use clap::Args;
use kako_mods_lib::local::{
    mcmod::cache::{MetadataRefresh, ModsCache},
    modloader::{LoaderId, ModLoaderMeta},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf};

#[derive(Debug, Args, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::commands) struct WriteArgs {
    /// Target cache on the file host; defaults to ~/.mcpm/cache/mods. Explicit directories must be initialized.
    #[arg(long, value_name = "PATH")]
    #[serde(default)]
    cache_dir: Option<PathBuf>,
    /// Copy, require a hard link, or copy only when hard links are unsupported.
    #[arg(long, value_enum, default_value = "copy")]
    #[serde(default)]
    mode: FileStrategy,
    /// Reidentify selected loaders even when current cached results exist.
    #[arg(long)]
    #[serde(default)]
    force: bool,
    /// Maximum concurrent files during name/ID scanning and cache import/identification.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    #[serde(default = "default_parallelism")]
    parallelism: NonZeroUsize,
}
impl WriteArgs {
    pub fn options(self, loaders: HashMap<LoaderId, ModLoaderMeta>) -> Result<Options> {
        Ok(Options {
            cache: match self.cache_dir {
                Some(path) => ModsCache::new(path),
                None => ModsCache::user_default()?,
            },
            loaders,
            mode: self.mode.into(),
            refresh: if self.force {
                MetadataRefresh::Force
            } else {
                MetadataRefresh::MissingOrOutdated
            },
            parallelism: self.parallelism.get(),
        })
    }
}
