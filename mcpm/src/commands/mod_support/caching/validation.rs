//! Cache-operation event validation shared by local and SSH presentation.

use super::{Event, Stage, Status};
use crate::commands::mod_support::scan::{self, ProgressState};
use anyhow::{Result, ensure};
use kako_mods_lib::local::mcmod::cache::CacheId;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Validator {
    selection: ProgressState,
    selection_errors: bool,
    progress: ProgressState,
    started: bool,
    completed: usize,
    total: usize,
    paths: HashSet<std::path::PathBuf>,
    objects: HashMap<CacheId, bool>,
    failed: usize,
    done: bool,
}
impl Validator {
    pub fn accept(&mut self, event: &Event) -> Result<()> {
        ensure!(!self.done, "cache data after completion");
        match event {
            Event::Selection(event) => {
                ensure!(!self.started, "cache selection after import started");
                match event {
                    scan::Event::Progress { completed, total } => {
                        self.selection.accept(*completed, *total)?
                    }
                    scan::Event::Diagnostic { .. } => self.selection_errors = true,
                }
            }
            Event::Progress { completed, total } => {
                ensure!(
                    self.selection.is_complete(),
                    "cache import before selection completed"
                );
                ensure!(
                    !self.started || *completed > 0,
                    "cache import started twice"
                );
                self.progress.accept(*completed, *total)?;
                self.started = true;
                self.completed = *completed;
                self.total = *total;
            }
            Event::File(file) => {
                ensure!(
                    self.started && self.paths.len() < self.completed,
                    "cache file before its import completed"
                );
                ensure!(
                    !file.path.as_os_str().is_empty() && self.paths.insert(file.path.clone()),
                    "empty or duplicate cache file path"
                );
                ensure!(
                    file.success == file.error.is_none()
                        && file.error.is_some() == file.failed_stage.is_some(),
                    "inconsistent cache file failure"
                );
                ensure!(
                    file.id.is_some() == file.published.is_some(),
                    "incomplete cache import outcome"
                );
                ensure!(
                    file.id.is_some() == file.status.is_some()
                        && file.id.is_some() == file.data_path.is_some()
                        && file.id.is_some() == file.digest_path.is_some(),
                    "incomplete cache entry observation"
                );
                ensure!(
                    matches!(file.status, Some(Status::Unavailable)) == file.reason.is_some(),
                    "inconsistent cache availability reason"
                );
                if let Some(id) = &file.id {
                    let id: CacheId = id.parse()?;
                    let published = file.published.unwrap();
                    ensure!(
                        published == file.method.is_some(),
                        "inconsistent cache transfer method"
                    );
                    ensure!(
                        !matches!(file.failed_stage, Some(Stage::Import)),
                        "import failure with successful import"
                    );
                    *self.objects.entry(id).or_default() |= published;
                } else {
                    ensure!(
                        matches!(file.failed_stage, Some(Stage::Import)) && file.method.is_none(),
                        "missing cache import without import failure"
                    );
                }
                ensure!(
                    !matches!(file.failed_stage, Some(Stage::Metadata)) || file.metadata_requested,
                    "metadata failure without requesting metadata"
                );
                for (source, result) in &file.identification {
                    ensure!(
                        !source.is_empty() && !source.contains('@'),
                        "invalid identification source key"
                    );
                    ensure!(
                        !matches!(result, super::record::IdentResult::Failed { .. })
                            || !file.success,
                        "failed identification source in successful cache result"
                    );
                }
                self.failed += usize::from(!file.success);
            }
            Event::Done(summary) => {
                ensure!(
                    self.selection.is_complete() && self.progress.is_complete(),
                    "cache ended during processing"
                );
                ensure!(
                    summary.files == self.paths.len() && summary.files == self.total,
                    "cache completion file count mismatch"
                );
                ensure!(
                    self.started || summary.files == 0,
                    "cache missing import progress"
                );
                ensure!(
                    summary.failed_files == self.failed
                        && (summary.selection_failures > 0) == self.selection_errors,
                    "cache failure counts disagree with diagnostics"
                );
                let added = self.objects.values().filter(|added| **added).count();
                ensure!(
                    summary.objects_added == added
                        && summary.objects_reused == self.objects.len() - added,
                    "cache object totals mismatch"
                );
                self.done = true;
            }
        }
        Ok(())
    }
    pub fn finish(&self) -> Result<()> {
        ensure!(self.done, "missing cache completion");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::mod_support::caching::{Record, Summary, record::Method};

    fn progress(completed: usize) -> Event {
        Event::Progress {
            completed,
            total: 2,
        }
    }
    fn file(path: &str, published: bool) -> Event {
        Event::File(Box::new(Record {
            path: path.into(),
            id: Some("a".repeat(64)),
            published: Some(published),
            method: published.then_some(Method::Copy),
            status: Some(Status::Available),
            reason: None,
            data_path: Some("cache/aa/object.dat".into()),
            digest_path: Some("cache/aa/object.data.json".into()),
            metadata_requested: true,
            identification: Default::default(),
            success: true,
            failed_stage: None,
            error: None,
            error_remote: false,
        }))
    }
    fn done() -> Event {
        Event::Done(Summary {
            files: 2,
            objects_added: 1,
            ..Default::default()
        })
    }
    #[test]
    fn validates_parallel_completions_ordered_files_and_distinct_object_totals() {
        let mut validator = Validator::default();
        for event in [
            progress(0),
            progress(1),
            progress(2),
            file("one.jar", false),
            file("two.jar", true),
            done(),
        ] {
            // Exercise the same codec used by the remote command.
            let event = serde_json::from_value(serde_json::to_value(event).unwrap()).unwrap();
            validator.accept(&event).unwrap();
        }
        validator.finish().unwrap();
        assert!(validator.accept(&progress(0)).is_err());
    }
    #[test]
    fn rejects_incomplete_or_inconsistent_operation_streams() {
        for events in [
            vec![file("one.jar", true)],
            vec![progress(0), progress(2)],
            vec![
                progress(0),
                progress(1),
                file("one.jar", true),
                file("two.jar", false),
            ],
            vec![
                progress(0),
                progress(1),
                progress(2),
                file("one.jar", true),
                file("one.jar", false),
            ],
            vec![progress(0), progress(1), progress(2), done()],
            vec![
                progress(0),
                progress(1),
                progress(2),
                file("one.jar", false),
                file("two.jar", false),
                done(),
            ],
        ] {
            let mut validator = Validator::default();
            assert!(events.iter().any(|event| validator.accept(event).is_err()));
        }
        let mut invalid = file("one.jar", true);
        if let Event::File(record) = &mut invalid {
            record.method = None;
        }
        let mut validator = Validator::default();
        validator.accept(&progress(0)).unwrap();
        validator.accept(&progress(1)).unwrap();
        assert!(validator.accept(&invalid).is_err());
        assert!(Validator::default().finish().is_err());
    }
}
