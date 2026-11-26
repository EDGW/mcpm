//! Boundary-aware URL-prefix mappings for compatible mirrors and custom endpoints.

use std::{collections::HashSet, fmt, sync::Arc};

use anyhow::{Result, ensure};
use url::Url;

use crate::download::{Catalog, DownloadSource, MappingTable, UrlMapping};

/// A named source with ordered mappings; equally specific mappings may provide
/// multiple addresses. Only mappings with the longest matching path apply.
#[derive(Clone)]
pub struct MappedSource {
    id: String,
    mappings: Vec<UrlMapping>,
    catalog: Option<Arc<dyn Catalog>>,
}

impl MappedSource {
    pub fn new(id: impl Into<String>, mappings: Vec<UrlMapping>) -> Result<Self> {
        let id = id.into();
        ensure!(!id.trim().is_empty(), "download source ID cannot be empty");
        Ok(Self {
            id,
            mappings,
            catalog: None,
        })
    }

    /// Attaches a mirror-specific catalog protocol to this source.
    pub fn with_catalog(mut self, catalog: Arc<dyn Catalog>) -> Self {
        self.catalog = Some(catalog);
        self
    }

    fn mapped(&self, url: &Url, reverse: bool) -> Vec<Url> {
        let matching: Vec<_> = self
            .mappings
            .iter()
            .filter_map(|mapping| {
                let (from, to) = if reverse {
                    (mapping.mirror(), mapping.original())
                } else {
                    (mapping.original(), mapping.mirror())
                };
                translate(url, from, to).map(|translated| (from.path().len(), translated))
            })
            .collect();
        let longest = matching.iter().map(|(length, _)| *length).max();
        let mut urls = Vec::new();
        for (length, url) in matching {
            if Some(length) == longest && !urls.contains(&url) {
                urls.push(url);
            }
        }
        urls
    }
}

impl fmt::Debug for MappedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MappedSource")
            .field("id", &self.id)
            .field("mappings", &self.mappings)
            .field("has_catalog", &self.catalog.is_some())
            .finish()
    }
}

impl DownloadSource for MappedSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn translate(&self, original: &Url) -> Result<Vec<Url>> {
        Ok(self.mapped(original, false))
    }

    fn original_url(&self, url: &Url) -> Result<Option<Url>> {
        let mut originals = self.mapped(url, true);
        Ok((originals.len() == 1).then(|| originals.remove(0)))
    }

    fn mapping_table(&self) -> Result<Option<MappingTable>> {
        if self.mappings.is_empty() {
            return Ok(None);
        }
        let mut seen = HashSet::with_capacity(self.mappings.len());
        let mut primary = Vec::with_capacity(self.mappings.len());
        for mapping in &self.mappings {
            if seen.insert(mapping.original()) {
                primary.push(mapping.clone());
            }
        }
        Ok(Some(MappingTable::new(primary)?))
    }

    fn catalog(&self) -> Option<&dyn Catalog> {
        self.catalog.as_deref()
    }
}

fn translate(url: &Url, from: &Url, to: &Url) -> Option<Url> {
    if url.origin() != from.origin() || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let suffix = url.path().strip_prefix(from.path())?;
    let mut translated = to.clone();
    translated.set_path(&format!("{}{suffix}", to.path()));
    translated.set_query(url.query());
    translated.set_fragment(url.fragment());
    Some(translated)
}
