//! Remote server selection, streamed scan diagnostics and validated archive paths.

use std::{num::NonZeroUsize, path::PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ServerMods;
use crate::{
    commands::mod_support::{
        scan,
        scan_output::ScanOutput,
        selection::{self, Selection, SelectionInput},
    },
    diagnostics::ErrorReport,
    remote::{
        client::Session,
        command::{Command, Output},
    },
};

struct Identify;
impl Command for Identify {
    const ID: &'static str = "server.mods.source.identify";

    #[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
    fn run(contents: Value, output: &Output) -> Result<()> {
        let args: Request =
            serde_json::from_value(contents).context("invalid server source identify arguments")?;
        let selection = args.selection.into_selection()?;
        ensure!(
            !selection.is_empty(),
            "server source identify requires mod selections"
        );
        tracing::info!(server = %args.server.display(), "Resolving remote server mods for source identification");
        output.checkpoint()?;
        let server = ServerMods::load(args.server)?;
        let (paths, failed) = selection::resolve_paths(
            selection,
            server.scope(),
            server.loaders(),
            args.parallelism.get(),
            || output.checkpoint(),
            |event| {
                output.send(match event {
                    scan::Event::Progress { completed, total } => Event::Scan { completed, total },
                    scan::Event::Diagnostic { error } => {
                        crate::logging::record_error(&error.clone().into_error());
                        Event::Error { error }
                    }
                })
            },
        )?;
        output.checkpoint()?;
        tracing::debug!(
            files = paths.len(),
            failed,
            "Remote server source selection completed"
        );
        output.send(Event::Done { paths, failed })
    }
}

/// Selection and concurrency only: provider specifications never cross this boundary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    server: PathBuf,
    selection: SelectionInput,
    #[serde(default = "crate::commands::execution::default_parallelism")]
    parallelism: NonZeroUsize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Scan { completed: usize, total: usize },
    Error { error: ErrorReport },
    Done { paths: Vec<PathBuf>, failed: usize },
}

pub(super) fn resolve(
    session: &Session,
    server: PathBuf,
    selection: Selection,
    parallelism: usize,
) -> Result<(Vec<PathBuf>, usize)> {
    let args = Request {
        server,
        selection: SelectionInput::from_selection(selection)?,
        parallelism: NonZeroUsize::new(parallelism)
            .context("identification parallelism must be positive")?,
    };
    let output = ScanOutput::remote();
    let mut validator = Validator::default();
    let mut result = None;
    session
        .submit(Identify::ID, serde_json::to_value(args)?)?
        .collect(|reply| {
            let event: Event = reply
                .decode()
                .context("invalid remote server source selection")?;
            validator.accept(&event)?;
            match event {
                Event::Scan { completed, total } => {
                    output.event(scan::Event::Progress { completed, total })
                }
                Event::Error { error } => output.event(scan::Event::Diagnostic { error }),
                Event::Done { paths, failed } => {
                    result = Some((paths, failed));
                    Ok(())
                }
            }
        })?;
    validator.finish()?;
    result.context("missing remote server source selection completion")
}

/// A directory scan must complete before selected paths can be consumed.
#[derive(Default)]
struct Validator {
    scan: scan::ProgressState,
    errors: bool,
    done: bool,
}
impl Validator {
    fn accept(&mut self, event: &Event) -> Result<()> {
        ensure!(!self.done, "server source selection data after completion");
        match event {
            Event::Scan { completed, total } => {
                self.scan.accept(*completed, *total)?;
            }
            Event::Error { .. } => self.errors = true,
            Event::Done { paths, failed } => {
                ensure!(
                    self.scan.is_complete(),
                    "server source selection ended during scan"
                );
                ensure!(
                    (*failed > 0) == self.errors,
                    "server source selection failures disagree with diagnostics"
                );
                let mut seen = std::collections::HashSet::new();
                ensure!(
                    paths
                        .iter()
                        .all(|path| !path.as_os_str().is_empty() && seen.insert(path)),
                    "empty or duplicate remote server mod path"
                );
                self.done = true;
            }
        }
        Ok(())
    }
    fn finish(self) -> Result<()> {
        ensure!(
            self.done,
            "missing remote server source selection completion"
        );
        Ok(())
    }
}

fn init() -> Result<()> {
    crate::remote::registry::register::<Identify>()
}
inventory::submit! { crate::initcall::Initcall(init) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_selection_lifecycle_and_recoverable_failure() {
        let mut validator = Validator::default();
        for event in [
            Event::Scan {
                completed: 0,
                total: 1,
            },
            Event::Error {
                error: ErrorReport::capture(&anyhow::anyhow!("bad archive")),
            },
            Event::Scan {
                completed: 1,
                total: 1,
            },
            Event::Done {
                paths: vec!["good.jar".into()],
                failed: 1,
            },
        ] {
            validator.accept(&event).unwrap();
        }
        validator.finish().unwrap();
    }

    #[test]
    fn rejects_incomplete_progress_duplicate_paths_and_terminal_events() {
        let done = || Event::Done {
            paths: Vec::new(),
            failed: 0,
        };
        let cases = [
            vec![Event::Scan {
                completed: 1,
                total: 2,
            }],
            vec![
                Event::Scan {
                    completed: 0,
                    total: 2,
                },
                done(),
            ],
            vec![
                Event::Scan {
                    completed: 0,
                    total: 2,
                },
                Event::Scan {
                    completed: 2,
                    total: 2,
                },
            ],
            vec![Event::Done {
                paths: vec!["a.jar".into(), "a.jar".into()],
                failed: 0,
            }],
            vec![Event::Done {
                paths: Vec::new(),
                failed: 1,
            }],
            vec![done(), done()],
        ];
        for events in cases {
            let mut validator = Validator::default();
            assert!(events.iter().any(|event| validator.accept(event).is_err()));
        }
        assert!(Validator::default().finish().is_err());
    }
}
