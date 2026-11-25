//! Download-source contract and static URL-prefix mapping types without transport behavior.

use std::collections::HashSet;

use anyhow::{Result, ensure};
use url::Url;

use super::Catalog;

/// A configured website's URL mapping rules. Implementations perform no I/O.
pub trait DownloadSource: Send + Sync {
    /// Stable identity used in configuration and diagnostics.
    fn id(&self) -> &str;

    /// Translates an original URL into this source's addresses, in attempt order.
    /// An empty list means unsupported. Errors indicate invalid rules or input.
    fn translate(&self, original: &Url) -> Result<Vec<Url>>;

    /// Restores a known mirror URL only when its original address is unambiguous.
    /// This lets target wrappers route URLs embedded in mirror metadata again.
    fn original_url(&self, _url: &Url) -> Result<Option<Url>> {
        Ok(None)
    }

    /// Expresses this source as an ordered static prefix rewrite table when possible.
    ///
    /// - `Ok(None)`: cannot be expressed as a static prefix table.
    /// - `Ok(Some(empty))`: explicit direct source with no prefix rewrites.
    /// - `Ok(Some(entries))`: ordered prefix rewrite rules (longest path prefix first).
    fn mapping_table(&self) -> Result<Option<MappingTable>> {
        Ok(None)
    }

    /// Optional mirror-specific version catalog protocol. Returning `None` uses
    /// the official catalog protocol translated through [`DownloadSource::translate`].
    fn catalog(&self) -> Option<&dyn Catalog> {
        None
    }
}

/// Ordered static URL-prefix rewrite rules with unique original prefixes, sorted
/// stably by original path length descending (longest path prefix first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingTable {
    entries: Vec<UrlMapping>,
}

impl MappingTable {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn new(mut entries: Vec<UrlMapping>) -> Result<Self> {
        {
            let mut seen = HashSet::with_capacity(entries.len());
            for entry in &entries {
                ensure!(
                    seen.insert(entry.original()),
                    "duplicate original URL prefix in mapping table: {}",
                    entry.original()
                );
            }
        }
        entries.sort_by(|a, b| b.original().path().len().cmp(&a.original().path().len()));
        Ok(Self { entries })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[UrlMapping] {
        &self.entries
    }
}

/// Original and mirror directory prefixes. Query strings and fragments belong
/// to the requested URL, never the mapping configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    pub fn original(&self) -> &Url {
        &self.original
    }

    pub fn mirror(&self) -> &Url {
        &self.mirror
    }
}
