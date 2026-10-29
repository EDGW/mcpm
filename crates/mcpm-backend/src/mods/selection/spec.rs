//! Structured selection inputs independent of Clap and operating-system string encoding.
//!
//! The spec is what crosses a process boundary; the operation converts it into ordered
//! queries whose values may be non-UTF-8 for a local target.

use std::ffi::OsString;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{Query, Selection};

/// Wire form of an ordered mod selection.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionSpec {
    #[serde(default)]
    queries: Vec<QuerySpec>,
    #[serde(default)]
    all: bool,
}

impl SelectionSpec {
    /// Converts a caller-owned selection; remote queries must be valid UTF-8.
    pub fn from_selection(selection: Selection) -> Result<Self> {
        Ok(Self {
            queries: selection
                .queries
                .into_iter()
                .map(|query| {
                    Ok(QuerySpec {
                        kind: query.kind,
                        value: query
                            .value
                            .into_string()
                            .ok()
                            .context("remote mod queries must be valid UTF-8")?,
                    })
                })
                .collect::<Result<_>>()?,
            all: selection.all,
        })
    }

    /// Rebuilds the ordered selection consumed by the resolver.
    pub fn into_selection(self) -> Result<Selection> {
        ensure!(
            !self.all || self.queries.is_empty(),
            "all cannot be combined with mod queries"
        );
        Ok(Selection {
            queries: self
                .queries
                .into_iter()
                .map(|query| Query {
                    kind: query.kind,
                    value: OsString::from(query.value),
                })
                .collect(),
            all: self.all,
        })
    }
}

/// One wire query: a kind and its value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuerySpec {
    kind: super::QueryKind,
    value: String,
}
