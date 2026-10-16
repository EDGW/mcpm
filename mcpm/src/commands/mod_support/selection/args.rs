//! Ordered Clap inputs; indices preserve interleaved automatic and explicit selectors.

use std::{ffi::OsString, path::PathBuf};

use anyhow::Result;
use clap::{ArgMatches, Args, Command, FromArgMatches, ValueEnum};
use thiserror::Error;

use super::{Query, QueryKind};

/// CLI and stdin queries in user-supplied order, or an explicit complete scan.
pub(in crate::commands) struct SelectionArgs {
    pub(super) queries: Vec<Query>,
    pub(super) all: bool,
}

impl SelectionArgs {
    pub(in crate::commands) fn with_stdin(self) -> Result<Self> {
        self.with_stdin_kind(QueryKind::Auto)
    }

    fn with_stdin_kind(mut self, kind: QueryKind) -> Result<Self> {
        let paths = super::super::stdin::append(Vec::new())?;
        if self.all && !paths.is_empty() {
            return Err(AllWithQueries.into());
        }
        self.queries.extend(paths.into_iter().map(|value| Query {
            kind,
            value: value.into_os_string(),
        }));
        Ok(self)
    }

    pub(in crate::commands::mod_support) fn into_queries(self) -> Option<Vec<Query>> {
        (!self.all).then_some(self.queries)
    }

    pub(in crate::commands) fn is_empty(&self) -> bool {
        !self.all && self.queries.is_empty()
    }
}

/// Interpretation of stdin lines, independent of positional and explicit CLI selectors.
#[derive(Args)]
pub(in crate::commands) struct StdinArgs {
    /// Interpret stdin lines as names, files, modids, or automatic filename/modid/name queries.
    #[arg(long, value_enum, default_value = "unspecified", value_name = "TYPE")]
    input: StdinKind,
}

impl StdinArgs {
    pub(in crate::commands) fn read(&self, selection: SelectionArgs) -> Result<SelectionArgs> {
        let kind = match self.input {
            StdinKind::Name => QueryKind::Name,
            StdinKind::File => QueryKind::File,
            StdinKind::Modid => QueryKind::ModId,
            StdinKind::Unspecified => QueryKind::Auto,
        };
        selection.with_stdin_kind(kind)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum StdinKind {
    Name,
    File,
    Modid,
    Unspecified,
}

#[derive(Args)]
struct RawSelectionArgs {
    /// Match each input by filename, then exact modid, then exact mod name.
    #[arg(value_name = "INPUT", num_args = 0..)]
    queries: Vec<OsString>,
    /// Inspect a file without falling back to modid or name; repeat to select more.
    #[arg(long = "file", value_name = "FILE", action = clap::ArgAction::Append)]
    file: Vec<PathBuf>,
    /// Match an exact modid; repeat to select more.
    #[arg(long, value_name = "MODID", action = clap::ArgAction::Append)]
    modid: Vec<String>,
    /// Match an exact mod display name; repeat to select more.
    #[arg(long, value_name = "NAME", action = clap::ArgAction::Append)]
    name: Vec<String>,
    /// Inspect every mod file in the selected directory.
    #[arg(long, conflicts_with_all = ["queries", "file", "modid", "name"])]
    all: bool,
}

impl FromArgMatches for SelectionArgs {
    fn from_arg_matches(matches: &ArgMatches) -> std::result::Result<Self, clap::Error> {
        let raw = RawSelectionArgs::from_arg_matches(matches)?;
        let mut queries = Vec::new();
        for (id, kind, values) in [
            ("queries", QueryKind::Auto, raw.queries),
            (
                "file",
                QueryKind::File,
                raw.file.into_iter().map(PathBuf::into_os_string).collect(),
            ),
            (
                "modid",
                QueryKind::ModId,
                raw.modid.into_iter().map(OsString::from).collect(),
            ),
            (
                "name",
                QueryKind::Name,
                raw.name.into_iter().map(OsString::from).collect(),
            ),
        ] {
            if let Some(indices) = matches.indices_of(id) {
                queries.extend(
                    indices
                        .zip(values)
                        .map(|(index, value)| (index, Query { kind, value })),
                );
            }
        }
        queries.sort_by_key(|(index, _)| *index);
        Ok(Self {
            queries: queries.into_iter().map(|(_, query)| query).collect(),
            all: raw.all,
        })
    }

    fn update_from_arg_matches(
        &mut self,
        matches: &ArgMatches,
    ) -> std::result::Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for SelectionArgs {
    fn group_id() -> Option<clap::Id> {
        RawSelectionArgs::group_id()
    }
    fn augment_args(command: Command) -> Command {
        RawSelectionArgs::augment_args(command)
    }
    fn augment_args_for_update(command: Command) -> Command {
        RawSelectionArgs::augment_args_for_update(command)
    }
}

#[derive(Debug, Error)]
#[error("--all cannot be combined with queries from stdin")]
struct AllWithQueries;
