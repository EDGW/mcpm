//! Shared explicit cache-write CLI policy and its transportable file-host options.

use super::Options;
use crate::commands::{cache_support::FileStrategy, execution::default_parallelism};
use anyhow::{Result, ensure};
use clap::{Args, ValueEnum};
use mcpm_backend::local::cache::mods::MetadataRefresh;
use mcpm_backend::local::modloader::LoaderId;
use mcpm_backend::local::modloader::ModLoaderMeta;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, num::NonZeroUsize};

#[derive(Debug, Args, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::commands) struct WriteArgs {
    /// Copy, require a hard link, or copy only when hard links are unsupported.
    #[arg(long, value_enum, default_value = "copy")]
    #[serde(default)]
    mode: FileStrategy,
    /// Select meta and/or ident, comma-separated; ',' selects neither.
    #[arg(long, value_name = "METAS", value_parser = parse_meta_list)]
    #[serde(default)]
    metas: Option<MetaList>,
    /// Select one meta kind; repeat or combine with --metas.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "META")]
    #[serde(default)]
    meta: Vec<Meta>,
    /// Import content only, without calculating meta or ident.
    #[arg(long, conflicts_with_all = ["metas", "meta"])]
    #[serde(default)]
    no_metas: bool,
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
    pub fn selected_metas(&self) -> Result<Vec<Meta>> {
        ensure!(
            !self.no_metas || (self.metas.is_none() && self.meta.is_empty()),
            "--no-metas conflicts with --metas and --meta"
        );
        if self.no_metas {
            return Ok(Vec::new());
        }
        let mut selected = self
            .metas
            .as_ref()
            .map(|metas| metas.0.clone())
            .unwrap_or_else(|| {
                if self.meta.is_empty() {
                    Meta::ALL.to_vec()
                } else {
                    Vec::new()
                }
            });
        selected.extend(self.meta.iter().copied());
        selected.sort_unstable();
        selected.dedup();
        Ok(selected)
    }
    pub fn force(&self) -> bool {
        self.force
    }
    pub fn parallelism(&self) -> usize {
        self.parallelism.get()
    }
    pub fn options(self, loaders: HashMap<LoaderId, ModLoaderMeta>) -> Result<Options> {
        let metadata = self.selected_metas()?.contains(&Meta::Meta);
        Ok(Options {
            loaders,
            mode: self.mode.into(),
            refresh: if self.force {
                MetadataRefresh::Force
            } else {
                MetadataRefresh::MissingOrOutdated
            },
            parallelism: self.parallelism.get(),
            metadata,
        })
    }
}

/// Independent result categories; selecting ident does not require loader metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::commands) enum Meta {
    Meta,
    Ident,
}
impl Meta {
    const ALL: [Self; 2] = [Self::Meta, Self::Ident];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
struct MetaList(Vec<Meta>);

fn parse_meta_list(value: &str) -> Result<MetaList, String> {
    value
        .split(',')
        .filter(|item| !item.is_empty())
        .map(|item| Meta::from_str(item, false))
        .collect::<Result<Vec<_>, _>>()
        .map(MetaList)
}
