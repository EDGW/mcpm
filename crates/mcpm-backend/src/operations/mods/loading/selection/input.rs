//! Portable selection encoding; native local paths are converted only when crossing transport.

use super::{Query, QueryKind, Selection};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug)]
pub struct SelectionInput {
    selection: Selection,
}
impl Default for SelectionInput {
    fn default() -> Self {
        Self {
            selection: Selection {
                queries: Vec::new(),
                all: false,
            },
        }
    }
}
impl SelectionInput {
    pub fn from_selection(selection: Selection) -> Result<Self> {
        ensure!(
            !selection.all || selection.queries.is_empty(),
            "all cannot be combined with mod queries"
        );
        Ok(Self { selection })
    }
    pub fn into_selection(self) -> Result<Selection> {
        Ok(self.selection)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    #[serde(default)]
    queries: Vec<Input>,
    #[serde(default)]
    all: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    kind: QueryKind,
    value: String,
}

impl Serialize for SelectionInput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let queries = self
            .selection
            .queries
            .iter()
            .map(|query| {
                Ok(Input {
                    kind: query.kind,
                    value: query
                        .value
                        .to_str()
                        .context("remote mod queries must be valid UTF-8")?
                        .to_owned(),
                })
            })
            .collect::<Result<_>>()
            .map_err(serde::ser::Error::custom)?;
        Wire {
            queries,
            all: self.selection.all,
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for SelectionInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        Self::from_selection(Selection {
            queries: wire
                .queries
                .into_iter()
                .map(|query| Query {
                    kind: query.kind,
                    value: query.value.into(),
                })
                .collect(),
            all: wire.all,
        })
        .map_err(serde::de::Error::custom)
    }
}
