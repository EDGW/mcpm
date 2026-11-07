//! Execution-host file summaries for literal paths or server-selected outer archives.

use super::loading::{
    ScopeInput, scan,
    selection::{SelectionInput, resolve_paths},
};
use crate::operations::stream::blocking_stream;
use anyhow::{Result, ensure};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::source::{FileSummary, SummaryRequirements};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Compute file summaries without reading or modifying Mod caches.
pub struct Digest;

impl Operation for Digest {
    const ID: &'static str = "mod.digest";
    type Input = DigestInput;
    type Output = StreamingOutput<DigestEvent>;

    async fn run(input: DigestInput) -> Result<Self::Output> {
        Ok(blocking_stream(move |sender| {
            let server_selection = matches!(&input.target, DigestTarget::Server { .. });
            let (selected, selection_failed) = match input.target {
                DigestTarget::Paths { paths } => (paths, 0),
                DigestTarget::Server { path, selection } => {
                    let selection = selection.into_selection()?;
                    ensure!(
                        !selection.is_empty(),
                        "provide server mod selections or --all"
                    );
                    sender.checkpoint()?;
                    let resolved = ScopeInput::Server {
                        path,
                        no_cache: true,
                    }
                    .resolve()?;
                    resolve_paths(
                        selection,
                        resolved.scope,
                        resolved.loaders,
                        &resolved.environment,
                        input.parallelism,
                        || sender.checkpoint(),
                        |event| sender.send(DigestEvent::Selection(event)),
                    )?
                }
            };
            let mut paths = Vec::with_capacity(selected.len());
            let mut positions = HashMap::new();
            let mut jobs: Vec<(PathBuf, Vec<usize>)> = Vec::new();
            for path in selected {
                sender.checkpoint()?;
                let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                let position = *positions.entry(key).or_insert_with(|| {
                    jobs.push((path.clone(), Vec::new()));
                    jobs.len() - 1
                });
                if server_selection && !jobs[position].1.is_empty() {
                    continue;
                }
                jobs[position].1.push(paths.len());
                paths.push(path);
            }
            let total = paths.len();
            sender.send(DigestEvent::Prepared { paths })?;
            let next = AtomicUsize::new(0);
            let failed = AtomicUsize::new(0);
            std::thread::scope(|scope| -> Result<()> {
                let mut workers = Vec::new();
                for _ in 0..input.parallelism.max(1).min(jobs.len()) {
                    let (sender, jobs, next, failed) = (&sender, &jobs, &next, &failed);
                    workers.push(scope.spawn(move || -> Result<()> {
                        loop {
                            sender.checkpoint()?;
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some((path, indices)) = jobs.get(index) else {
                                break;
                            };
                            sender.send(DigestEvent::Loading {
                                indices: indices.clone(),
                            })?;
                            let result = FileSummary::read(path, input.requirements);
                            if result.is_err() {
                                failed.fetch_add(indices.len(), Ordering::Relaxed);
                            }
                            sender.send(DigestEvent::Item {
                                indices: indices.clone(),
                                outcome: result.map_err(PortableError::from),
                            })?;
                        }
                        Ok(())
                    }));
                }
                for worker in workers {
                    worker
                        .join()
                        .map_err(|_| anyhow::anyhow!("file summary worker panicked"))??;
                }
                Ok(())
            })?;
            sender.send(DigestEvent::Done {
                total,
                unique: jobs.len(),
                failed: failed.load(Ordering::Relaxed),
                selection_failed,
            })
        }))
    }
}

/// Execution-host target and the fingerprints requested for each unique file.
#[derive(Debug, Serialize, Deserialize)]
pub struct DigestInput {
    pub target: DigestTarget,
    pub requirements: SummaryRequirements,
    pub parallelism: usize,
}

/// Literal paths retain input positions; server selections resolve to unique outer files.
#[derive(Debug, Serialize, Deserialize)]
pub enum DigestTarget {
    Paths {
        paths: Vec<PathBuf>,
    },
    Server {
        path: PathBuf,
        selection: SelectionInput,
    },
}

/// Selection diagnostics followed by file lifecycles mapped to the prepared paths.
#[derive(Debug, Serialize, Deserialize)]
pub enum DigestEvent {
    Selection(scan::Event),
    Prepared {
        paths: Vec<PathBuf>,
    },
    Loading {
        indices: Vec<usize>,
    },
    Item {
        indices: Vec<usize>,
        outcome: Result<FileSummary, PortableError>,
    },
    Done {
        /// Number of original input positions.
        total: usize,
        /// Number of distinct execution-host file jobs.
        unique: usize,
        /// Number of original input positions whose file summary failed.
        failed: usize,
        /// Diagnosed query and lazy-index scan failures, separate from summary failures.
        selection_failed: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn server_selections_hash_opaque_files_once_and_preserve_mixed_failures() {
        use super::super::loading::selection::{Query, QueryKind, Selection};
        use mcpm_lib::local::mc::server::{
            MinecraftServer, MinecraftServerMetadata, SERVER_METADATA_VERSION, ServerModLoader,
        };

        let root = tempfile::tempdir().unwrap();
        MinecraftServer::create(
            root.path(),
            MinecraftServerMetadata {
                version: SERVER_METADATA_VERSION,
                minecraft_version: "1.21.1".into(),
                loader: Some(ServerModLoader {
                    loader_type: "fabric".into(),
                    version: "0.16.0".into(),
                }),
                start: None,
            },
        )
        .unwrap();
        let file = root.path().join("mods/opaque.jar");
        std::fs::write(&file, b"abc").unwrap();
        let selection = SelectionInput::from_selection(Selection {
            queries: [
                "opaque.jar",
                "mods/opaque.jar",
                "../outside.jar",
                "missing.jar",
            ]
            .into_iter()
            .map(|value| Query {
                kind: QueryKind::File,
                value: value.into(),
            })
            .collect(),
            all: false,
        })
        .unwrap();
        let mut stream = Digest::run(DigestInput {
            target: DigestTarget::Server {
                path: root.path().to_owned(),
                selection,
            },
            requirements: SummaryRequirements::default(),
            parallelism: 2,
        })
        .await
        .unwrap();
        let mut prepared = None;
        let mut outcomes = Vec::new();
        let mut selection_errors = 0;
        let mut done = None;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                DigestEvent::Selection(scan::Event::Diagnostic { .. }) => selection_errors += 1,
                DigestEvent::Prepared { paths } => prepared = Some(paths),
                DigestEvent::Item { indices, outcome } => outcomes.push((indices, outcome)),
                DigestEvent::Done {
                    total,
                    unique,
                    failed,
                    selection_failed,
                } => done = Some((total, unique, failed, selection_failed)),
                _ => {}
            }
        }
        assert_eq!(
            prepared.unwrap(),
            vec![file, root.path().join("mods/missing.jar")]
        );
        assert_eq!(done, Some((2, 2, 1, 1)));
        assert_eq!(selection_errors, 1);
        outcomes.sort_by(|left, right| left.0.cmp(&right.0));
        let summary = outcomes[0].1.as_ref().unwrap();
        assert_eq!(summary.size, 3);
        assert_eq!(summary.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert!(outcomes[1].1.is_err());
    }

    #[tokio::test]
    async fn duplicate_inputs_keep_all_positions_and_failures_do_not_stop_other_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.bin");
        std::fs::write(&path, b"abc").unwrap();
        let missing = root.path().join("missing.bin");
        let paths = vec![path.clone(), missing.clone(), path, missing];
        let mut stream = Digest::run(DigestInput {
            target: DigestTarget::Paths {
                paths: paths.clone(),
            },
            requirements: SummaryRequirements { murmur2: true },
            parallelism: 2,
        })
        .await
        .unwrap();
        let mut items = Vec::new();
        let mut loading = Vec::new();
        let mut done = None;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                DigestEvent::Prepared { paths: prepared } => assert_eq!(prepared, paths),
                DigestEvent::Loading { indices } => loading.push(indices),
                DigestEvent::Item { indices, outcome } => items.push((indices, outcome)),
                DigestEvent::Done {
                    total,
                    unique,
                    failed,
                    selection_failed,
                } => done = Some((total, unique, failed, selection_failed)),
                DigestEvent::Selection(_) => unreachable!("literal paths do not select mods"),
            }
        }
        assert_eq!(done, Some((4, 2, 2, 0)));
        assert_eq!(items.len(), 2);
        loading.sort();
        assert_eq!(loading, vec![vec![0, 2], vec![1, 3]]);
        let success = items
            .iter()
            .find(|(indices, _)| indices == &[0, 2])
            .unwrap();
        let summary = success.1.as_ref().unwrap();
        assert_eq!(summary.size, 3);
        assert_eq!(summary.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert!(summary.murmur2.is_some());
        let failure = items
            .into_iter()
            .find(|(indices, _)| indices == &[1, 3])
            .unwrap();
        assert!(failure.1.unwrap_err().as_inner().chain().count() > 1);
    }

    #[tokio::test]
    async fn summaries_keep_sha1_bytes_but_filter_fingerprint_whitespace_and_reject_directories() {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        let spaced = root.path().join("spaced");
        let empty = root.path().join("empty");
        std::fs::write(&plain, b"abc").unwrap();
        std::fs::write(&spaced, b"a\tb\nc\r ").unwrap();
        std::fs::write(&empty, b"").unwrap();
        let mut stream = Digest::run(DigestInput {
            target: DigestTarget::Paths {
                paths: vec![plain, spaced, empty, root.path().to_owned()],
            },
            requirements: SummaryRequirements { murmur2: true },
            parallelism: 1,
        })
        .await
        .unwrap();
        let mut summaries = Vec::new();
        while let Some(event) = stream.next().await {
            if let DigestEvent::Item { outcome, .. } = event.unwrap() {
                summaries.push(outcome);
            }
        }
        let plain = summaries[0].as_ref().unwrap();
        let spaced = summaries[1].as_ref().unwrap();
        let empty = summaries[2].as_ref().unwrap();
        assert_eq!(plain.murmur2, spaced.murmur2);
        assert_ne!(plain.sha1, spaced.sha1);
        assert_eq!(spaced.size, 7);
        assert_eq!(empty.size, 0);
        assert_eq!(empty.sha1, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert!(summaries[3].is_err());
    }

    #[tokio::test]
    async fn unrequested_fingerprints_are_absent_and_host_aliases_share_work() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        std::fs::write(&path, b"abc").unwrap();
        let mut stream = Digest::run(DigestInput {
            target: DigestTarget::Paths {
                paths: vec![path, root.path().join(".").join("file")],
            },
            requirements: SummaryRequirements::default(),
            parallelism: 2,
        })
        .await
        .unwrap();
        let mut items = 0;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                DigestEvent::Item { indices, outcome } => {
                    items += 1;
                    assert_eq!(indices, vec![0, 1]);
                    assert!(outcome.unwrap().murmur2.is_none());
                }
                DigestEvent::Done {
                    total,
                    unique,
                    failed,
                    selection_failed,
                } => {
                    assert_eq!((total, unique, failed, selection_failed), (2, 1, 0, 0));
                }
                _ => {}
            }
        }
        assert_eq!(items, 1);
    }
}
