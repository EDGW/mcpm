//! File-level cache reuse totals shared by local acquisition and remote completion events.

use mcpm_backend::local::mcmod::ModCacheReuse;
use serde::{Deserialize, Serialize};

/// Counts independent inspections, including errors and unrecognized files.
/// Duplicate queries, nested declarations and presentation filters do not change it.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub(in crate::commands) struct CacheSummary {
    pub fully_cached: usize,
    pub partially_cached: usize,
    pub total: usize,
}

impl CacheSummary {
    pub(super) fn from_results(results: impl IntoIterator<Item = ModCacheReuse>) -> Self {
        let mut summary = Self::default();
        for reuse in results {
            summary.total += 1;
            match reuse {
                ModCacheReuse::Full => summary.fully_cached += 1,
                ModCacheReuse::Partial => summary.partially_cached += 1,
                ModCacheReuse::None => {}
            }
        }
        summary
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.fully_cached <= self.total
                && self.partially_cached <= self.total - self.fully_cached,
            "invalid cache summary: cached item counts exceed total"
        );
        Ok(())
    }
}
