//! Validated BLAKE3 or whole-inventory selection shared by cache batch commands.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use clap::Args;
use mcpm_lib::local::cache::hash::Blake3Hash;

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
}

impl From<Selection> for mcpm_backend::operations::cache::Selection {
    fn from(selection: Selection) -> Self {
        match selection {
            Selection::Ids(ids) => Self::Ids(ids),
            Selection::All => Self::All,
        }
    }
}
