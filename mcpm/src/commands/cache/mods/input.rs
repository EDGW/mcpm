//! Validated BLAKE3 or whole-inventory selection shared by cache batch commands.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use clap::Args;
use kako_mods_lib::local::cache::{hash::Blake3Hash, mods::ModsCacheDir};

#[derive(Debug, Args)]
pub(super) struct Inputs {
    /// Complete BLAKE3 IDs; redirected stdin appends one ID per nonblank line.
    #[arg(value_name = "HASH", num_args = 0.., conflicts_with = "all")]
    ids: Vec<Blake3Hash>,
    /// Select every cache entry; conflicts with HASH arguments and nonblank stdin.
    #[arg(long, global = true)]
    all: bool,
}

impl Inputs {
    pub(super) fn read(self) -> Result<Selection> {
        let lines = crate::commands::stdin::lines()?;
        if self.all {
            ensure!(
                self.ids.is_empty() && lines.is_empty(),
                "--all cannot be combined with HASH arguments or nonblank stdin"
            );
            return Ok(Selection::All);
        }
        let mut ids = self.ids;
        for line in lines {
            ids.push(
                line.parse()
                    .with_context(|| format!("invalid BLAKE3 ID from stdin: {line:?}"))?,
            );
        }
        ensure!(
            !ids.is_empty(),
            "provide BLAKE3 IDs as arguments or one per stdin line, or use --all"
        );
        let mut seen = HashSet::new();
        ids.retain(|id| seen.insert(*id));
        Ok(Selection::Ids(ids))
    }
}

/// Input conflicts are checked before opening the cache or starting any batch work.
pub(super) enum Selection {
    Ids(Vec<Blake3Hash>),
    All,
}

impl Selection {
    pub(super) fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    pub(super) fn resolve(self, cache: &ModsCacheDir) -> Result<Vec<Blake3Hash>> {
        match self {
            Self::Ids(ids) => Ok(ids),
            Self::All => Ok(cache
                .scan()
                .context("failed to select all cache entries")?
                .into_iter()
                .map(|entry| entry.key)
                .collect()),
        }
    }
}
