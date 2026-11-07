//! Adapts the Host identification stream to per-source list presentation and file-level facts.

use crate::commands::source_support::identify::Location;
use anyhow::Result;
use mcpm_backend::operations::mods::{
    identification::{QueryResult, SourceResolution},
    source::{
        Configuration,
        identify::{Event, Identify, Input, Target},
    },
};
use mcpm_lib::source::{FileMatch, MatchedFileRole};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, channel},
    },
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
    Matched {
        file: FileMatch,
        cached: bool,
    },
    Unmatched,
    Skipped,
    Failed {
        error: crate::diagnostics::ErrorReport,
        remote: bool,
    },
}
impl State {
    pub(super) fn details(&self, spinner: &str) -> String {
        match self {
            Self::Queued => "Waiting".into(),
            Self::Loading => format!("{spinner} Identifying…"),
            Self::Unmatched => "No match".into(),
            Self::Skipped => "Not queried".into(),
            Self::Failed { error, remote } => {
                crate::display::error::lines(error, *remote).join("\n")
            }
            Self::Matched { file, cached } => format!(
                "✓ Project ID: {} · File ID: {} · {} · {} · {} bytes · {}",
                file.reference.project.id.0,
                file.reference.id.0,
                file.filename,
                match file.role {
                    MatchedFileRole::Primary => "Primary",
                    MatchedFileRole::Additional => "Additional",
                },
                file.size,
                if *cached { "Cache" } else { "Query" },
            ),
        }
    }

    pub(super) fn for_source(
        source: &mcpm_backend::operations::mods::identification::SourceResult,
    ) -> Self {
        match &source.resolution {
            SourceResolution::Cached(file) => Self::Matched {
                file: file.clone(),
                cached: true,
            },
            SourceResolution::Queried(Some(file)) => Self::Matched {
                file: file.clone(),
                cached: false,
            },
            SourceResolution::Queried(None) => Self::Unmatched,
            SourceResolution::Failed(error) => {
                let (error, remote) = crate::diagnostics::capture(error.as_inner());
                Self::Failed { error, remote }
            }
            SourceResolution::Skipped => Self::Skipped,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Failure {
    pub error: crate::diagnostics::ErrorReport,
    pub remote: bool,
}
impl Failure {
    fn capture(error: &anyhow::Error) -> Self {
        let (error, remote) = crate::diagnostics::capture(error);
        Self { error, remote }
    }
    pub(super) fn state(&self) -> State {
        State::Failed {
            error: self.error.clone(),
            remote: self.remote,
        }
    }
}

#[derive(Debug)]
pub(super) enum Update {
    Loading {
        files: Vec<usize>,
    },
    Item {
        files: Vec<usize>,
        result: std::result::Result<Arc<QueryResult>, Failure>,
    },
    /// Transport/cancellation is lifecycle failure, not a fabricated source query result.
    Stopped {
        files: Vec<usize>,
        failure: Failure,
    },
}

/// The TUI consumes the same Host operation as the source identify command.
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
    let executor = location.executor.clone();
    let input = Input {
        target: Target::Paths {
            paths,
            cache: location.cache.cloned(),
        },
        sources: Platform::ALL
            .into_iter()
            .map(|platform| Configuration::from(platform.id()))
            .collect(),
        force: false,
        parallelism,
    };
    tokio::spawn(
        async move {
            let mut completed = vec![false; count];
            let result: Result<()> = async {
                let mut stream = executor.call::<Identify>(input).await?;
                let mut done = false;
                while let Some(event) = stream.next().await.transpose()? {
                    match event {
                        Event::Prepared { diagnostics, .. } => {
                            for error in diagnostics {
                                crate::diagnostics::error(error.as_inner());
                            }
                        }
                        Event::Loading { indices } => {
                            sender.send(Update::Loading { files: indices })?
                        }
                        Event::Item { indices, result } => {
                            for &index in &indices {
                                completed[index] = true;
                            }
                            sender.send(Update::Item {
                                files: indices,
                                result: result
                                    .map(Arc::new)
                                    .map_err(|error| Failure::capture(error.as_inner())),
                            })?;
                        }
                        Event::Done(summary) => {
                            done = true;
                            tracing::debug!(
                                ?summary,
                                "Mod list received identification completion"
                            );
                        }
                    }
                }
                anyhow::ensure!(done, "missing identification completion");
                Ok(())
            }
            .await;
            if let Err(error) = result {
                crate::logging::record_error(&error);
                let _ = sender.send(Update::Stopped {
                    files: (0..count).filter(|&index| !completed[index]).collect(),
                    failure: Failure::capture(&error),
                });
            }
        }
        .in_current_span(),
    );
    Ok(receiver)
}
