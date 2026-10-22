//! Bounded concurrent outer-JAR source lookups and provider-neutral result states for the TUI.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
};

use tracing::Instrument;

use kako_mods_lib::source::{FileMatch, MatchedFileRole, Source};

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
    Unavailable,
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
            Self::Unavailable => "Unavailable for SSH data".into(),
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
pub(super) struct Update {
    pub files: Vec<usize>,
    pub platform: Platform,
    pub state: State,
}

/// Runs on the TUI's runtime; dropping that runtime cancels pending network I/O.
pub(super) fn start(paths: Vec<PathBuf>, parallelism: usize) -> Receiver<Update> {
    let (sender, receiver) = channel();
    tracing::info!(
        files = paths.len(),
        parallelism,
        "Starting mod source identification"
    );
    tokio::spawn(async move {
        let sources = Platform::ALL.map(|platform| {
            crate::commands::source_support::build_source(platform.id())
                .map(Arc::<dyn Source>::from)
                .map_err(|error| format!("{error:#}"))
        });
        for (platform, source) in Platform::ALL.into_iter().zip(&sources) {
            if let Err(error) = source { tracing::debug!(platform = platform.id(), %error, "Source provider initialization failed"); }
        }
        identify(paths, &sources, &sender, parallelism).await;
    }.in_current_span());
    receiver
}

/// Keep first-occurrence order and share results for duplicate physical paths.
async fn identify(
    paths: Vec<PathBuf>,
    sources: &[Result<Arc<dyn Source>, String>; 2],
    sender: &Sender<Update>,
    parallelism: usize,
) {
    let started = std::time::Instant::now();
    let count = paths.len();
    let mut positions = HashMap::new();
    let mut jobs: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let position = *positions.entry(key).or_insert_with(|| {
            jobs.push((path, Vec::new()));
            jobs.len() - 1
        });
        jobs[position].1.push(index);
    }
    tracing::debug!(
        files = count,
        unique_files = jobs.len(),
        "Prepared source identification jobs"
    );
    let mut jobs = jobs.into_iter().flat_map(|(path, files)| {
        Platform::ALL
            .into_iter()
            .map(move |platform| (path.clone(), files.clone(), platform))
    });
    let mut active = tokio::task::JoinSet::new();
    loop {
        while active.len() < parallelism.max(1) {
            let Some((path, files, platform)) = jobs.next() else {
                break;
            };
            if sender
                .send(Update {
                    files: files.clone(),
                    platform,
                    state: State::Loading,
                })
                .is_err()
            {
                return;
            }
            let source = sources[platform.index()].clone();
            let sender = sender.clone();
            let span = tracing::debug_span!("identify_source", path = %path.display(), platform = platform.id());
            active.spawn(
                async move {
                    let started = std::time::Instant::now();
                    tracing::debug!("Looking up mod source");
                    let state = match source {
                        Ok(source) => match source.identify_file(&path).await {
                            Ok(Some(found)) => State::Matched(found),
                            Ok(None) => State::Unmatched,
                            Err(error) => State::Failed(format!("{error:#}")),
                        },
                        Err(error) => State::Failed(error),
                    };
                    tracing::debug!(
                        outcome = match &state {
                            State::Matched(_) => "matched",
                            State::Unmatched => "unmatched",
                            State::Failed(_) => "failed",
                            _ => "unexpected",
                        },
                        elapsed_ms = started.elapsed().as_millis(),
                        "Mod source lookup completed"
                    );
                    if let State::Failed(error) = &state {
                        tracing::debug!(%error, "Mod source lookup failed");
                    }
                    let _ = sender.send(Update {
                        files,
                        platform,
                        state,
                    });
                }
                .instrument(span),
            );
        }
        if active.join_next().await.is_none() {
            break;
        }
    }
    tracing::info!(
        files = count,
        elapsed_ms = started.elapsed().as_millis(),
        "Mod source identification completed"
    );
}

#[cfg(test)]
mod tests;
