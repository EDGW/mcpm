//! Host metadata and source refresh; network requests never cross a caller-side stage.

use super::{CacheInput, Entry, IdentSnapshot, ReadOptions, Selection, observe};
use crate::operations::stream::blocking_stream;
use anyhow::{Context, Result};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::{
    cache::{
        hash::Blake3Hash,
        mods::{MetadataRefresh, ModsCacheDir, meta::CacheDigest},
    },
    modloader::{LoaderId, ModLoaderMeta},
};
use serde::{Deserialize, Serialize};
mod batch;
mod ident;
pub use ident::{Ident, IdentEvent, IdentInput, IdentObservation, IdentReceipt};

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaInput {
    pub cache: CacheInput,
    pub selection: Selection,
    pub loaders: Vec<String>,
    pub force: bool,
    pub parallelism: usize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum MetaEvent {
    Selected(usize),
    Entry {
        id: Blake3Hash,
        result: Result<MetaReceipt, PortableError>,
    },
    Done,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct MetaReceipt {
    pub content: Entry,
    pub digest: Option<CacheDigest>,
}

pub struct Meta;
impl Operation for Meta {
    const ID: &'static str = "cache.refresh_meta";
    type Input = MetaInput;
    type Output = StreamingOutput<MetaEvent>;
    async fn run(input: MetaInput) -> Result<Self::Output> {
        let (cache, ids, loaders) = tokio::task::spawn_blocking(move || {
            let cache = super::protection::open(input.cache, true, &[])?;
            let ids = input.selection.resolve(&cache)?;
            let loaders = loaders(input.loaders)?;
            Ok::<_, anyhow::Error>((cache, ids, loaders))
        })
        .await??;
        Ok(blocking_stream(move |sender| {
            sender.send(MetaEvent::Selected(ids.len()))?;
            batch::run(
                &ids,
                input.parallelism,
                || sender.checkpoint(),
                |id| {
                    let entry = cache
                        .refresh_metadata(
                            id,
                            &loaders,
                            if input.force {
                                MetadataRefresh::Force
                            } else {
                                MetadataRefresh::MissingOrOutdated
                            },
                        )
                        .with_context(|| format!("failed to refresh Loader metadata for {id}"))?;
                    Ok(MetaReceipt {
                        digest: entry.digest().cloned(),
                        content: snapshot(&cache, id)?,
                    })
                },
                |id, result| {
                    let failed = result.is_err();
                    sender.send(MetaEvent::Entry {
                        id,
                        result: result.map_err(Into::into),
                    })?;
                    Ok(!failed)
                },
            )?;
            sender.send(MetaEvent::Done)
        }))
    }
}

fn loaders(names: Vec<String>) -> Result<std::collections::HashMap<LoaderId, ModLoaderMeta>> {
    let available = &mcpm_lib::local::modloader::BUILTIN_LOADERS;
    if names.is_empty() {
        return Ok(mcpm_lib::local::modloader::BUILTIN_LOADERS.clone());
    }
    let mut selected = std::collections::HashMap::new();
    for name in names {
        let (id, loader) = available
            .iter()
            .find(|(id, _)| id.as_str() == name)
            .with_context(|| format!("unknown loader {name:?}"))?;
        selected.insert(id.clone(), loader.clone());
    }
    Ok(selected)
}
fn snapshot(cache: &ModsCacheDir, id: Blake3Hash) -> Result<Entry> {
    observe(cache, id, cache.inspect(&id), ReadOptions::default())
}
