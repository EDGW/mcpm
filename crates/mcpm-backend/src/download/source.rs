//! Download-source contract for URL translation without metadata or transport behavior.

use anyhow::Result;
use url::Url;

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
}
