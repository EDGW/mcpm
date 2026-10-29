//! Explicit name/UUID selection, literal stdin merging and UUID-level deduplication.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use mcpm_backend::local::mc::server::stash::{Selector, Stash, StashStore};
use uuid::Uuid;

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub(super) struct Single {
    /// Select one exact stash name; commas are literal.
    #[arg(long)]
    name: Option<String>,
    /// Select one complete stash UUID.
    #[arg(long)]
    id: Option<Uuid>,
}

impl Single {
    pub fn selector(self) -> Selector {
        match self.id {
            Some(id) => Selector::Id(id),
            None => Selector::Name(self.name.expect("clap requires a selector")),
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InputType {
    Name,
    Id,
}

#[derive(Debug, Args)]
pub(super) struct Multiple {
    /// Exact name; repeat to select more. Commas are literal.
    #[arg(long)]
    name: Vec<String>,
    /// Comma-separated names, combined with --name.
    #[arg(long, value_delimiter = ',')]
    names: Vec<String>,
    /// Complete UUID; repeat to select more.
    #[arg(long)]
    id: Vec<Uuid>,
    /// Comma-separated UUIDs, combined with --id.
    #[arg(long, value_delimiter = ',')]
    ids: Vec<Uuid>,
    /// Interpret nonblank stdin lines explicitly; no automatic type inference.
    #[arg(long, value_enum)]
    input: Option<InputType>,
}

impl Multiple {
    pub fn read(&self) -> Result<Vec<Selector>> {
        let mut selected: Vec<_> = self
            .name
            .iter()
            .cloned()
            .chain(self.names.iter().cloned())
            .map(Selector::Name)
            .chain(self.id.iter().chain(&self.ids).copied().map(Selector::Id))
            .collect();
        for line in crate::commands::stdin::lines()? {
            selected.push(
                match self
                    .input
                    .context("nonblank stdin requires --input name or --input id")?
                {
                    InputType::Name => Selector::Name(line),
                    InputType::Id => Selector::Id(
                        line.parse()
                            .with_context(|| format!("invalid stash UUID from stdin: {line:?}"))?,
                    ),
                },
            );
        }
        ensure!(
            !selected.is_empty(),
            "select stashes with --name/--id or typed stdin"
        );
        let mut seen = HashSet::new();
        selected.retain(|selector| seen.insert(selector.clone()));
        Ok(selected)
    }
}

pub(super) fn resolve(
    store: &StashStore<'_>,
    selectors: Vec<Selector>,
) -> Vec<(String, Result<Stash>)> {
    let mut seen = HashSet::new();
    selectors
        .into_iter()
        .filter_map(|selector| {
            let label = label(&selector);
            let result = store
                .resolve(&selector)
                .with_context(|| format!("failed to resolve stash {label:?}"));
            if result.as_ref().is_ok_and(|stash| !seen.insert(stash.id())) {
                None
            } else {
                Some((label, result))
            }
        })
        .collect()
}

pub(super) fn label(selector: &Selector) -> String {
    match selector {
        Selector::Id(id) => id.to_string(),
        Selector::Name(name) => name.clone(),
    }
}
