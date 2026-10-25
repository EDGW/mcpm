//! Bounded concurrent outer-JAR source lookups and provider-neutral result states for the TUI.

use crate::commands::source_support::identify::{self as acquisition, Event, Location, Outcome};
use anyhow::Result;
use mcpm_backend::source::{FileMatch, MatchedFileRole};
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, channel},
};
use tracing::Instrument;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Platform {
    Modrinth,
    CurseForge,
}
impl Platform {
    pub(super) const ALL: [Self; 2] = [Self::Modrinth, Self::CurseForge];

    pub(super) fn index(self) -> usize {
        match self {
            Self::Modrinth => 0,
            Self::CurseForge => 1,
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Modrinth => "modrinth",
            Self::CurseForge => "curseforge",
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Modrinth => "Modrinth",
            Self::CurseForge => "CurseForge",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) enum State {
    #[default]
    Queued,
    Loading,
    Matched(FileMatch),
    Unmatched,
    Failed(String),
}
impl State {
    pub(super) fn details(&self, spinner: &str) -> String {
        match self {
            Self::Queued => "Waiting".into(),
            Self::Loading => format!("{spinner} Identifying…"),
            Self::Unmatched => "No match".into(),
            Self::Failed(error) => format!("Failed: {error}"),
            Self::Matched(found) => format!(
                "✓ Project ID: {} · File ID: {} · {} · {} · {} bytes",
                found.reference.project.id.0,
                found.reference.id.0,
                found.filename,
                match found.role {
                    MatchedFileRole::Primary => "Primary",
                    MatchedFileRole::Additional => "Additional",
                },
                found.size,
            ),
        }
    }
}

#[derive(Debug)]
pub(super) enum Update {
    State {
        files: Vec<usize>,
        platform: Platform,
        state: State,
    },
    Cache {
        files: Vec<usize>,
        sources: Vec<Platform>,
    },
}

/// The TUI adapts shared identification events without owning lookup scheduling.
pub(super) fn start(
    paths: Vec<PathBuf>,
    parallelism: usize,
    location: Location<'_>,
) -> Result<Receiver<Update>> {
    let (sender, receiver) = channel();
    let count = paths.len();
    if count == 0 {
        return Ok(receiver);
    }
    let sources = Platform::ALL.map(|platform| platform.id().to_owned());
    let identification =
        acquisition::Identification::start(paths, &sources, parallelism, location)?;
    tracing::info!(
        files = count,
        parallelism,
        "Starting mod source identification"
    );
    tokio::spawn(async move {
        let mut completed = vec![[false; 2]; count];
        let mut emit = |event| {
            if let Event::Item { indices, source, .. } = &event {
                for index in indices { completed[*index][*source] = true; }
            }
            send_update(event, &sender)
        };
        let result = identification.run(&mut emit).await;
        if let Err(error) = result {
            tracing::warn!(error = %format_args!("{error:#}"), "Mod source identification stopped");
            for platform in Platform::ALL {
                let _ = sender.send(Update::State { files: (0..count).filter(|index| !completed[*index][platform.index()]).collect(), platform, state: State::Failed(format!("{error:#}")) });
            }
        }
    }.in_current_span());
    Ok(receiver)
}

fn send_update(event: Event, sender: &std::sync::mpsc::Sender<Update>) -> Result<()> {
    let (indices, source, state) = match event {
        Event::Cache { indices, sources } => {
            sender.send(Update::Cache {
                files: indices,
                sources: sources
                    .into_iter()
                    .map(|index| Platform::ALL[index])
                    .collect(),
            })?;
            return Ok(());
        }
        Event::Loading { indices, source } => (indices, source, State::Loading),
        Event::Item {
            indices,
            source,
            outcome,
        } => (
            indices,
            source,
            match outcome {
                Outcome::Matched { file } => State::Matched(file),
                Outcome::Unmatched => State::Unmatched,
                Outcome::Failed { error, remote } => {
                    let prefix = if remote { "remote error: " } else { "" };
                    State::Failed(format!("{prefix}{:#}", error.into_error()))
                }
            },
        ),
        Event::Done { failed } => {
            tracing::debug!(
                failed,
                "Mod list received all source identification results"
            );
            return Ok(());
        }
    };
    sender.send(Update::State {
        files: indices,
        platform: Platform::ALL[source],
        state,
    })?;
    Ok(())
}
