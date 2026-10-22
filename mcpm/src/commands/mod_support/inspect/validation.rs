//! Inspection event lifecycle validation, independent of transport and presentation.

use anyhow::{Result, ensure};

use super::Event;

/// One state machine validates both local acquisition and remote event streams.
#[derive(Default)]
pub(in crate::commands) struct Validator {
    scan: Option<(usize, usize)>,
    files: usize,
    errors: bool,
    done: bool,
}

impl Validator {
    pub(in crate::commands) fn accept(&mut self, event: &Event) -> Result<()> {
        ensure!(!self.done, "mod inspection data after completion");
        match event {
            Event::Scan { completed, total } => {
                ensure!(completed <= total, "mod inspection progress exceeds total");
                match self.scan {
                    None => ensure!(*completed == 0, "mod inspection progress missing start"),
                    Some((previous, expected)) => ensure!(
                        *total == expected && *completed == previous + 1,
                        "invalid mod inspection scan progress sequence"
                    ),
                }
                self.scan = (completed < total).then_some((*completed, *total));
            }
            Event::Error { .. } => self.errors = true,
            Event::File { index, file } => {
                ensure!(
                    self.scan.is_none(),
                    "mod inspection file before scan completion"
                );
                ensure!(*index == self.files, "mod inspection file index mismatch");
                ensure!(
                    file.failed_loaders.is_empty() || self.errors,
                    "mod loader failures without diagnostics"
                );
                self.files += 1;
            }
            Event::Done { files, failed, .. } => {
                ensure!(
                    self.scan.is_none(),
                    "mod inspection ended with incomplete scan progress"
                );
                ensure!(*files == self.files, "mod inspection file count mismatch");
                // One failed archive can produce several loader diagnostics.
                ensure!(
                    (*failed > 0) == self.errors,
                    "mod inspection failure count disagrees with diagnostics"
                );
                self.done = true;
            }
        }
        Ok(())
    }

    pub(in crate::commands) fn finish(self) -> Result<()> {
        ensure!(self.done, "missing mod inspection completion");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commands::mod_support::inspect::File, diagnostics::ErrorReport};

    fn scan(completed: usize, total: usize) -> Event {
        Event::Scan { completed, total }
    }

    fn file(index: usize, failed: bool) -> Event {
        Event::File {
            index,
            file: File {
                path: "mod.jar".into(),
                filename: "mod.jar".into(),
                enabled: true,
                metadata: Vec::new(),
                failed_loaders: if failed {
                    vec!["fabric".into()]
                } else {
                    Vec::new()
                },
            },
        }
    }

    fn error() -> Event {
        Event::Error {
            error: ErrorReport::capture(&anyhow::anyhow!("archive failure")),
        }
    }

    fn done(files: usize, failed: usize) -> Event {
        Event::Done {
            files,
            failed,
            ignored: 0,
        }
    }

    #[test]
    fn accepts_scan_diagnostics_empty_batches_and_cached_selections() {
        let mut validator = Validator::default();
        for event in [
            scan(0, 2),
            error(),
            scan(1, 2),
            scan(2, 2),
            file(0, true),
            file(1, true),
            scan(0, 0),
            done(2, 1),
        ] {
            validator.accept(&event).unwrap();
        }
        validator.finish().unwrap();
    }

    #[test]
    fn rejects_broken_lifecycle_counts_and_progress_sequences() {
        let cases = [
            vec![scan(1, 2)],
            vec![scan(0, 2), scan(0, 2)],
            vec![scan(0, 2), scan(1, 3)],
            vec![scan(0, 2), scan(2, 2)],
            vec![scan(0, 1), scan(2, 1)],
            vec![scan(0, 1), file(0, false)],
            vec![scan(0, 1), done(0, 0)],
            vec![file(1, false)],
            vec![file(0, true)],
            vec![file(0, false), done(0, 0)],
            vec![error(), done(0, 0)],
            vec![done(0, 1)],
            vec![done(0, 0), scan(0, 0)],
            vec![done(0, 0), error()],
            vec![done(0, 0), file(0, false)],
            vec![done(0, 0), done(0, 0)],
        ];
        for events in cases {
            let mut validator = Validator::default();
            assert!(
                events.iter().any(|event| validator.accept(event).is_err()),
                "{events:?}"
            );
        }
        let mut incomplete = Validator::default();
        incomplete.accept(&scan(0, 0)).unwrap();
        assert!(incomplete.finish().is_err());
    }
}
