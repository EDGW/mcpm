//! Shared execution-host summary, verified reuse and content-checked publication.

use anyhow::{Context, Result, ensure};
use mcpm_lib::{
    local::{cache::mods::ModsCacheDir, runtime::RuntimeEnvironment},
    source::{FileMatch, FileSummary, SummaryRequirements},
};
use std::{collections::BTreeMap, path::Path};

mod model;
pub use model::*;
mod sources;
pub(crate) use sources::Sources;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) struct PreparedIdentification {
    pub summary: FileSummary,
    pub cached: BTreeMap<String, FileMatch>,
}

pub(crate) fn prepare(
    path: &Path,
    requirements: SummaryRequirements,
    sources: &[String],
    cache: Option<(&ModsCacheDir, &str)>,
    force: bool,
) -> Result<PreparedIdentification> {
    let summary = FileSummary::read(path, requirements)
        .with_context(|| format!("failed to summarize file {path:?}"))?;
    let cached = match cache {
        Some((cache, key)) if !force => cache
            .try_ident_info(key.parse()?)?
            .verified_matches(&summary),
        _ => BTreeMap::new(),
    };
    Ok(PreparedIdentification {
        summary,
        cached: selected(cached, sources),
    })
}

pub(crate) fn prepare_environment(
    path: &Path,
    requirements: SummaryRequirements,
    sources: &[String],
    environment: &RuntimeEnvironment,
    force: bool,
) -> Result<PreparedIdentification> {
    let summary = FileSummary::read(path, requirements)
        .with_context(|| format!("failed to summarize file {path:?}"))?;
    let cached = if force {
        BTreeMap::new()
    } else {
        match environment.mods_cache()? {
            Some(lookup) => lookup
                .find_ident_info(path)?
                .map(|info| info.verified_matches(&summary))
                .unwrap_or_default(),
            None => BTreeMap::new(),
        }
    };
    Ok(PreparedIdentification {
        summary,
        cached: selected(cached, sources),
    })
}
fn selected(
    mut cached: BTreeMap<String, FileMatch>,
    sources: &[String],
) -> BTreeMap<String, FileMatch> {
    cached.retain(|key, _| sources.iter().any(|source| source == key));
    cached
}

pub(crate) fn publish(
    cache: &ModsCacheDir,
    key: &str,
    expected: &FileSummary,
    matches: Vec<FileMatch>,
) -> Result<()> {
    let key = key.parse()?;
    cache
        .verified_size(&key)?
        .context("source identification requires cached content")?;
    let actual = FileSummary::read(
        &cache.content_path(&key),
        SummaryRequirements {
            murmur2: expected.murmur2.is_some(),
        },
    )?;
    ensure!(
        actual.size == expected.size
            && actual.sha1.eq_ignore_ascii_case(&expected.sha1)
            && actual.murmur2 == expected.murmur2,
        "cached content changed after identification preparation"
    );
    cache
        .verified_size(&key)?
        .context("cached content disappeared after identification preparation")?;
    for file in matches {
        file.validate(&actual)?;
        cache.write_ident_info(key, &file)?;
    }
    Ok(())
}
