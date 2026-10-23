//! Structured selection inputs independent of Clap and operating-system string encoding.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{Query, QueryKind, Selection};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::commands) struct SelectionInput {
    #[serde(default)]
    queries: Vec<Input>,
    #[serde(default)]
    all: bool,
}

impl SelectionInput {
    pub(in crate::commands) fn from_selection(args: Selection) -> Result<Self> {
        Ok(Self {
            queries: args
                .queries
                .into_iter()
                .map(|query| {
                    Ok(Input {
                        kind: query.kind,
                        value: query
                            .value
                            .into_string()
                            .ok()
                            .context("remote mod queries must be valid UTF-8")?,
                    })
                })
                .collect::<Result<_>>()?,
            all: args.all,
        })
    }

    pub(in crate::commands) fn into_selection(self) -> Result<Selection> {
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
                    value: query.value.into(),
                })
                .collect(),
            all: self.all,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    kind: QueryKind,
    value: String,
}
