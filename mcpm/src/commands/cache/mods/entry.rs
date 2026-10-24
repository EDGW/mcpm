//! Cache observation for selected fields; decoding remains in the Mod cache owner.

use anyhow::{Context, Result};
use kako_mods_lib::local::cache::{
    CacheFileKind, CacheInspection,
    hash::Blake3Hash,
    mods::{CacheStatus, ModsCacheDir, ident, meta},
};

use crate::display::cache::{Column, Entry, Metadata};

pub(super) fn observe(
    cache: &ModsCacheDir,
    id: Blake3Hash,
    inspection: Result<CacheInspection>,
    columns: &[Column],
    metadata_details: bool,
) -> Result<Entry> {
    let file = cache.content_path(&id);
    let mut entry = Entry {
        id,
        file,
        status: "unavailable",
        reason: None,
        size: None,
        sha256: None,
        metadatas: Vec::new(),
    };
    let inspection =
        inspection.with_context(|| format!("failed to inspect cached content {id}"))?;
    match inspection.content {
        Some(CacheFileKind::File { size }) => {
            entry.status = "available";
            entry.size = Some(size);
        }
        None => entry.status = "not_found",
        Some(_) => {
            entry.status = "unavailable";
            entry.reason = Some("cached content is not a regular file".into());
        }
    }
    let show_metadata = columns.contains(&Column::Metadatas);
    let decode_metadata = show_metadata && metadata_details;
    let meta = (columns.contains(&Column::Sha256)
        || (decode_metadata
            && inspection
                .metadata
                .iter()
                .any(|item| item.name == meta::NAME)))
    .then(|| cache.try_mod_metadata(id))
    .transpose()
    .with_context(|| format!("failed to observe Loader metadata for {id}"))?;
    entry.sha256 = meta
        .as_ref()
        .filter(|meta| meta.status() == &CacheStatus::Available)
        .and_then(|meta| meta.digest())
        .map(|digest| digest.sha256().to_owned());
    if show_metadata {
        let mut meta = meta;
        for item in inspection.metadata {
            if !metadata_details {
                entry.metadatas.push(Metadata::Other(item.name));
                continue;
            }
            entry.metadatas.push(match item.name.as_str() {
                meta::NAME => Metadata::Meta(meta.take().expect("meta was requested")),
                ident::NAME => Metadata::Ident(cache.try_ident_info(id).with_context(|| {
                    format!("failed to observe source identification for {id}")
                })?),
                _ => Metadata::Other(item.name),
            });
        }
    }
    Ok(entry)
}
