//! Shared explicit cache-write CLI policy and its transportable file-host options.

use super::Options;
use crate::commands::{cache_support::FileStrategy, execution::default_parallelism};
use anyhow::Result;
use clap::{Args, ValueEnum};
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
    /// Results to populate; data files are always imported.
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        default_value = "metadata,ident"
    )]
    #[serde(default = "default_contents")]
    contents: Vec<Content>,
    /// Reidentify requested results even when current cached results exist.
    #[arg(long)]
    #[serde(default)]
    force: bool,
    /// Maximum concurrent files during name/ID scanning and cache import/identification.
    #[arg(long, value_name = "COUNT", default_value_t = default_parallelism())]
    #[serde(default = "default_parallelism")]
    parallelism: NonZeroUsize,
}
impl WriteArgs {
    pub fn identification(&self) -> bool {
        self.contents.contains(&Content::Ident)
    }
    pub fn force(&self) -> bool {
        self.force
    }
    pub fn parallelism(&self) -> usize {
        self.parallelism.get()
    }
    pub fn cache_selection(&self) -> kako_mods_lib::local::mcmod::cache::CacheSelection {
        use kako_mods_lib::local::mcmod::cache::CacheSelection;
        match &self.cache_dir {
            Some(path) => CacheSelection::Directory { path: path.clone() },
            None => CacheSelection::Default,
        }
    }
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
            metadata: self.contents.contains(&Content::Metadata),
        })
    }
}

/// Independent result categories; selecting ident does not require loader metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Content {
    Metadata,
    Ident,
}
fn default_contents() -> Vec<Content> {
    vec![Content::Metadata, Content::Ident]
}

/// Provider specifications stay on the client and are never serialized to the file host.
#[derive(Args)]
pub(in crate::commands) struct SourcesArgs {
    /// Sources to identify: modrinth, curseforge, or curseforge@KEY; defaults to all sources.
    #[arg(
        long,
        value_name = "SOURCE",
        value_delimiter = ',',
        default_value = "modrinth,curseforge"
    )]
    pub sources: Vec<String>,
}
impl std::fmt::Debug for SourcesArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.sources
                    .iter()
                    .map(|s| s.split('@').next().unwrap_or(s)),
            )
            .finish()
    }
}
