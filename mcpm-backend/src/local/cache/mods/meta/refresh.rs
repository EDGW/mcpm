//! Provenance-checked Loader refresh and crate-private metadata commits.

use super::super::MetadataRefresh;
use super::{
    CACHE_FORMAT_VERSION, CacheDigest, CacheEntry, MOD_METADATA_VERSION, codec::METANAME,
    observation,
};
use crate::local::{
    cache::{CacheContent, CacheDir, hash::Blake3Hash},
    codec::Codec,
    mcmod::{CachedModMeta, ModEntry, ModEntryName, ModMeta},
    modloader::{LoaderId, ModLoaderMeta},
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, HashMap};

fn prepare(cache: &CacheDir, key: Blake3Hash) -> Result<(CacheContent, CacheDigest)> {
    let content = cache
        .open_content(&key)?
        .with_context(|| format!("cache object {key} does not exist"))?;
    let sha256 = observation::sha256(&content)?;
    let existing = content.try_read_metadata::<CacheDigest>(METANAME)?;
    let stored = existing.is_some();
    let mut digest = existing.unwrap_or_else(|| CacheDigest {
        version: CACHE_FORMAT_VERSION,
        blake3: key,
        size: content.size(),
        sha256: sha256.clone(),
        metadata_version: MOD_METADATA_VERSION,
        loaders: BTreeMap::new(),
    });
    ensure!(
        digest.valid_content_descriptor()
            && digest.blake3 == key
            && digest.size == content.size()
            && digest.sha256 == sha256,
        "cached Loader descriptor does not match content {key}"
    );
    if digest.metadata_version != MOD_METADATA_VERSION {
        digest.loaders.clear();
        digest.metadata_version = MOD_METADATA_VERSION;
    } else if !digest.loaders.is_empty() {
        ensure!(
            digest.valid_metadata(),
            "invalid current Loader metadata for {key}"
        );
    } else {
        // An existing empty snapshot is invalid; an absent snapshot may start empty.
        ensure!(!stored, "empty stored Loader metadata for {key}");
    }
    content.validate()?;
    Ok((content, digest))
}

pub(super) fn refresh(
    cache: &CacheDir,
    key: Blake3Hash,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    mode: MetadataRefresh,
) -> Result<CacheEntry> {
    if loaders.is_empty() {
        return Ok(observation::read(cache, key));
    }
    let (content, digest) = prepare(cache, key)?;
    let entry = ModEntry::from_parts(content.path().to_owned(), ModEntryName::new("cache.jar")?);
    let mut selected: Vec<_> = loaders.values().collect();
    selected.sort_by_key(|loader| loader.id());
    let mut metadata = HashMap::new();
    for loader in selected {
        if mode == MetadataRefresh::MissingOrOutdated
            && digest.loaders.contains_key(loader.id().as_str())
        {
            continue;
        }
        let result = loader.instance().try_identify(&entry).with_context(|| {
            format!(
                "loader {:?} failed to identify cached content {key}",
                loader.id()
            )
        })?;
        metadata.insert(loader.id().clone(), result);
    }
    content.validate()?;
    // Crate-private trusted commit: these results came from the exact pinned bytes.
    write_metadata(cache, key, &digest.sha256, &metadata).map(|(entry, _)| entry)
}

/// Never expose outside this crate: callers must establish exact-byte provenance.
pub(crate) fn write_metadata(
    cache: &CacheDir,
    key: Blake3Hash,
    expected_sha256: &str,
    metadata: &HashMap<LoaderId, Option<ModMeta>>,
) -> Result<(CacheEntry, bool)> {
    let (content, mut digest) = prepare(cache, key)?;
    ensure!(
        digest.sha256 == expected_sha256,
        "Loader metadata provenance does not match content {key}"
    );
    if metadata.is_empty() {
        return Ok((observation::read(cache, key), false));
    }
    for (loader, meta) in metadata {
        let captured = meta.as_ref().map(CachedModMeta::capture).transpose()?;
        ensure!(
            captured
                .as_ref()
                .is_none_or(|meta| meta.loader == loader.as_str()),
            "Loader metadata ownership does not match {loader:?}"
        );
        digest.loaders.insert(loader.as_str().to_owned(), captured);
    }
    ensure!(digest.valid_metadata(), "invalid Loader metadata for {key}");
    let old = content.try_read_metadata::<CacheDigest>(METANAME)?;
    let changed = old.as_ref().map(Codec::encode).transpose()?.as_deref()
        != Some(digest.encode()?.as_slice());
    content.validate()?;
    if changed {
        cache
            .write_metadata(&key, METANAME, &digest)?
            .context("content disappeared during Loader metadata commit")?;
    }
    content.validate()?;
    Ok((observation::read(cache, key), changed))
}
