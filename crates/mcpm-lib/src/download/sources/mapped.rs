//! Boundary-aware URL-prefix mappings for compatible mirrors and custom endpoints.

use anyhow::{Result, ensure};
use url::Url;

use crate::download::DownloadSource;

/// A named source with ordered mappings; equally specific mappings may provide
/// multiple addresses. Only mappings with the longest matching path apply.
#[derive(Debug, Clone)]
pub struct MappedSource {
    id: String,
    mappings: Vec<UrlMapping>,
}

impl MappedSource {
    pub fn new(id: impl Into<String>, mappings: Vec<UrlMapping>) -> Result<Self> {
        let id = id.into();
        ensure!(!id.trim().is_empty(), "download source ID cannot be empty");
        Ok(Self { id, mappings })
    }

    fn mapped(&self, url: &Url, reverse: bool) -> Vec<Url> {
        let matching: Vec<_> = self
            .mappings
            .iter()
            .filter_map(|mapping| {
                let (from, to) = if reverse {
                    (&mapping.mirror, &mapping.original)
                } else {
                    (&mapping.original, &mapping.mirror)
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
}

/// Original and mirror directory prefixes. Query strings and fragments belong
/// to the requested URL, never the mapping configuration.
#[derive(Debug, Clone)]
pub struct UrlMapping {
    original: Url,
    mirror: Url,
}

impl UrlMapping {
    pub fn new(original: Url, mirror: Url) -> Result<Self> {
        for prefix in [&original, &mirror] {
            ensure!(
                matches!(prefix.scheme(), "http" | "https")
                    && prefix.host_str().is_some()
                    && prefix.username().is_empty()
                    && prefix.password().is_none()
                    && prefix.query().is_none()
                    && prefix.fragment().is_none()
                    && prefix.path().ends_with('/'),
                "URL mapping requires an HTTP(S) directory prefix without credentials, query, or fragment: {prefix}"
            );
        }
        Ok(Self { original, mirror })
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
