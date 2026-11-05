//! Host-local cache management operations; no Mod identification or provider requests run here.

use anyhow::{Context, Result};
use mcpm_exec::operation::{Operation, StreamingOutput};
use mcpm_lib::local::cache::{codec::CACHE_DIRECTORY_VERSION, mods::ModsCacheDir};

mod model;
pub use model::*;
mod observe;
pub use observe::observe;
mod protection;
use super::stream;
pub use protection::check_protection;
mod verify;
pub use verify::Verify;

/// Initialize a missing/empty cache or validate an existing one.
pub struct Init;
impl Operation for Init {
    const ID: &'static str = "cache.init";
    type Input = CacheInput;
    type Output = InitOutput;
    async fn run(input: CacheInput) -> Result<InitOutput> {
        tokio::task::spawn_blocking(move || {
            let selection = check_protection(input.selection, true, &[], input.no_protect_stash)?;
            let cache = ModsCacheDir::initialize(selection.path()?)?;
            Ok(InitOutput {
                directory: cache.path().into(),
                version: CACHE_DIRECTORY_VERSION,
            })
        })
        .await?
    }
}

/// Return the full structural inventory without decoding named metadata.
pub struct List;
impl Operation for List {
    const ID: &'static str = "cache.list";
    type Input = CacheInput;
    type Output = ListOutput;
    async fn run(input: CacheInput) -> Result<ListOutput> {
        tokio::task::spawn_blocking(move || {
            let cache = protection::open(input, false, &[])?;
            Ok(ListOutput {
                directory: cache.path().into(),
                entries: cache
                    .scan()?
                    .into_iter()
                    .map(|entry| ListEntry {
                        key: entry.key,
                        file: cache.content_path(&entry.key),
                        inspection: entry.inspection.map(Into::into).map_err(Into::into),
                    })
                    .collect(),
            })
        })
        .await?
    }
}

/// Observe selected entries in order; unexpected errors end the stream after prior entries.
pub struct Info;
impl Operation for Info {
    const ID: &'static str = "cache.info";
    type Input = InfoInput;
    type Output = StreamingOutput<Entry>;
    async fn run(input: InfoInput) -> Result<Self::Output> {
        // Fail cache opening/selection before the caller publishes query headers.
        let (cache, ids) = tokio::task::spawn_blocking(move || {
            let cache = protection::open(input.cache, false, &[])?;
            let ids = input.selection.resolve(&cache)?;
            Ok::<_, anyhow::Error>((cache, ids))
        })
        .await??;
        Ok(stream::blocking_stream(move |sender| {
            for id in ids {
                sender.checkpoint()?;
                sender.send(observe(&cache, id, cache.inspect(&id), input.options)?)?;
            }
            Ok(())
        }))
    }
}

/// Import content only; already published bytes are reused and prior receipts survive failure.
pub struct Import;
impl Operation for Import {
    const ID: &'static str = "cache.import";
    type Input = ImportInput;
    type Output = StreamingOutput<ImportReceipt>;
    async fn run(input: ImportInput) -> Result<Self::Output> {
        Ok(stream::blocking_stream(move |sender| {
            let cache = protection::open(input.cache, true, &[])?;
            for path in input.paths {
                sender.checkpoint()?;
                let outcome = cache
                    .import_with_outcome(&path, input.mode.into())
                    .with_context(|| format!("failed to import {path:?}"))?;
                sender.send(ImportReceipt {
                    id: outcome.key,
                    file: cache.content_path(&outcome.key),
                    published: outcome.published,
                    method: outcome.method.map(Into::into),
                })?;
            }
            Ok(())
        }))
    }
}

/// Export in order, after protecting every destination before the first write.
pub struct Export;
impl Operation for Export {
    const ID: &'static str = "cache.export";
    type Input = ExportInput;
    type Output = StreamingOutput<ExportReceipt>;
    async fn run(input: ExportInput) -> Result<Self::Output> {
        Ok(stream::blocking_stream(move |sender| {
            let cache = protection::open(input.cache, false, &input.destinations)?;
            for destination in input.destinations {
                sender.checkpoint()?;
                let method = cache
                    .export_file(&input.id, &destination, input.mode.into())
                    .with_context(|| format!("failed to export {} to {destination:?}", input.id))?;
                sender.send(ExportReceipt {
                    id: input.id,
                    destination,
                    method: method.into(),
                })?;
            }
            Ok(())
        }))
    }
}

/// Delete explicit cache-owned keys; the frontend freezes remove-all before confirmation.
pub struct Remove;
impl Operation for Remove {
    const ID: &'static str = "cache.remove";
    type Input = RemoveInput;
    type Output = StreamingOutput<RemovalReceipt>;
    async fn run(input: RemoveInput) -> Result<Self::Output> {
        Ok(stream::blocking_stream(move |sender| {
            let selection = check_protection(
                input.cache.selection,
                true,
                &[],
                input.cache.no_protect_stash,
            )?;
            let cache = match input.directory {
                Some(path) => protection::open(
                    CacheInput {
                        selection: mcpm_lib::local::cache::mods::ModsCacheSelection::Directory {
                            path,
                        },
                        no_protect_stash: input.cache.no_protect_stash,
                    },
                    true,
                    &[],
                )?,
                None => selection.open()?,
            };
            for id in input.ids {
                sender.checkpoint()?;
                let removed = cache
                    .remove(&id)
                    .with_context(|| format!("failed to remove {id}"))?;
                sender.send(RemovalReceipt { id, removed })?;
            }
            Ok(())
        }))
    }
}

/// Rebuild only the optional lookup index, retaining the library's validation rules.
pub struct RebuildIndex;
impl Operation for RebuildIndex {
    const ID: &'static str = "cache.rebuild_index";
    type Input = CacheInput;
    type Output = usize;
    async fn run(input: CacheInput) -> Result<usize> {
        tokio::task::spawn_blocking(move || protection::open(input, true, &[])?.rebuild_index())
            .await?
    }
}
