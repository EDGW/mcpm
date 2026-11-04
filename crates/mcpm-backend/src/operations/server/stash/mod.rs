//! Host stash operations: one leased business session per call, with transport-only snapshots.

use std::collections::HashSet;

use anyhow::{Context, Result};
use mcpm_lib::local::mc::server::stash::{Stash, StashStore};

mod model;
pub use model::*;
mod query;
pub use query::{Delete, Info, List, Rename};
mod streaming;
pub use streaming::{Create, Status, Switch, Verify};

/// Resolve under the existing lease, preserving input order and the first selector per UUID.
fn resolve(store: &StashStore<'_>, selectors: Vec<Selector>) -> Vec<(Selector, Result<Stash>)> {
    let mut seen = HashSet::new();
    selectors
        .into_iter()
        .filter_map(|selector| {
            let label = match &selector {
                Selector::Name(name) => name.clone(),
                Selector::Id(id) => id.to_string(),
            };
            let result = store
                .resolve(&(&selector).into())
                .with_context(|| format!("failed to resolve stash {label:?}"));
            if result.as_ref().is_ok_and(|stash| !seen.insert(stash.id())) {
                None
            } else {
                Some((selector, result))
            }
        })
        .collect()
}
