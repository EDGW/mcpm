//! Client-owned source enrichment after file-host import; providers and credentials never travel.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use mcpm_backend::local::cache::mods::ModsCacheSelection;
use mcpm_backend::source::FileMatch;

use super::{
    Event, Record, Stage, Summary,
    args::{Meta, WriteArgs},
    record::IdentResult,
};
use crate::{
    commands::{
        execution::{Runtime, blocking},
        source_support::identify::{self, Identification, Location, Outcome, Sources},
    },
    diagnostics::ErrorReport,
};

mod remote;

/// Captures the storage phase only when source enrichment is requested.
/// The input protocol is validated before paths or IDs are used for client requests.
pub(in crate::commands) struct Completion {
    enabled: bool,
    cache: ModsCacheSelection,
    force: bool,
    parallelism: usize,
    sources: Sources,
    keys: Vec<String>,
    files: Vec<Record>,
    summary: Option<Summary>,
    validator: super::validation::Validator,
}
impl Completion {
    pub fn new(writing: &WriteArgs, specifications: &[String]) -> Result<Self> {
        let enabled = writing.selected_metas()?.contains(&Meta::Ident);
        let sources = if enabled {
            identify::prepare_sources(specifications)
        } else {
            Vec::new()
        };
        let keys: Vec<_> = sources
            .iter()
            .zip(specifications)
            .map(|(source, spec)| {
                source
                    .as_ref()
                    .map(|source| source.identification_cache_id().0)
                    .unwrap_or_else(|_| spec.split('@').next().unwrap_or(spec).to_owned())
            })
            .collect();
        let mut unique = HashSet::new();
        ensure!(
            !enabled || !keys.is_empty(),
            "select at least one source for identification caching"
        );
        for key in &keys {
            ensure!(
                !key.is_empty() && unique.insert(key.clone()),
                "select each identification source only once"
            );
        }
        Ok(Self {
            enabled,
            cache: writing.cache_selection(),
            force: writing.force(),
            parallelism: writing.parallelism(),
            sources,
            keys,
            files: Vec::new(),
            summary: None,
            validator: Default::default(),
        })
    }

    pub fn accept(
        &mut self,
        event: Event,
        emit: &mut impl FnMut(Event) -> Result<()>,
    ) -> Result<()> {
        self.validator.accept(&event)?;
        if !self.enabled {
            return emit(event);
        }
        match event {
            Event::File(file) => self.files.push(*file),
            Event::Done(summary) => self.summary = Some(summary),
            other => emit(other)?,
        }
        Ok(())
    }

    /// Files remain bounded in flight; provider lookups run on the client even for SSH files.
    pub fn finish(
        mut self,
        location: Location<'_>,
        mut emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<()> {
        self.validator.finish()?;
        if !self.enabled {
            return Ok(());
        }
        let mut summary = self.summary.take().context("missing storage completion")?;
        let mut pending: Vec<Option<Record>> = (0..self.files.len()).map(|_| None).collect();
        let mut next = 0;
        let mut completed = 0;
        summary.failed_files = 0;
        emit(Event::Progress {
            stage: Stage::Ident,
            completed,
            total: self.files.len(),
        })?;
        tracing::info!(
            files = self.files.len(),
            sources = self.keys.len(),
            "Caching source identification results on the file host"
        );
        // One reactor must outlive every shared HTTP connection. Blocking workers
        // may drive futures through its Handle, while this thread keeps I/O alive.
        Runtime::new()?.with_workers(|handle| {
            blocking::run(
                self.files.len(),
                self.parallelism,
                |index| self.process(self.files[index].clone(), location, handle),
                || Ok(()),
                |index, file| {
                    completed += 1;
                    summary.failed_files += usize::from(!file.success);
                    emit(Event::Progress {
                        stage: Stage::Ident,
                        completed,
                        total: self.files.len(),
                    })?;
                    pending[index] = Some(file);
                    while next < pending.len() {
                        let Some(file) = pending[next].take() else {
                            break;
                        };
                        emit(Event::File(Box::new(file)))?;
                        next += 1;
                    }
                    Ok(())
                },
            )
        })?;
        emit(Event::StageDone {
            stage: Stage::Ident,
            success: summary.failed_files == 0,
        })?;
        emit(Event::Done(summary))
    }

    fn process(
        &self,
        mut record: Record,
        location: Location<'_>,
        runtime: &tokio::runtime::Handle,
    ) -> Record {
        let Some(path) = record.data_path.clone() else {
            return record;
        };
        let operation = (|| -> Result<()> {
            let identification = Identification::with_sources_cached(
                vec![path],
                self.sources.clone(),
                1,
                location,
                if self.force {
                    None
                } else {
                    Some(self.cache.clone())
                },
            )?;
            let mut outcomes = vec![None; self.keys.len()];
            runtime.block_on(identification.run(|event| {
                if let identify::Event::Item {
                    source, outcome, ..
                } = event
                {
                    outcomes[source] = Some(outcome);
                }
                Ok(())
            }))?;
            for (source_index, (key, outcome)) in self.keys.iter().zip(outcomes).enumerate() {
                let outcome = match outcome.context("missing source identification result")? {
                    Outcome::Matched { file } => {
                        let publication = self.sources[source_index]
                            .as_ref()
                            .map_err(|error| error.clone().into_error())
                            .and_then(|source| file.for_cache(source.as_ref()))
                            .and_then(|file| self.publish(&record, &file, location));
                        match publication {
                            Ok(()) => IdentResult::Matched,
                            Err(error) => IdentResult::Failed {
                                error: ErrorReport::capture(&error),
                                remote: !location.is_local(),
                            },
                        }
                    }
                    Outcome::Unmatched => IdentResult::Unmatched,
                    Outcome::Failed { error, remote } => IdentResult::Failed { error, remote },
                };
                if let IdentResult::Failed { error, remote } = &outcome {
                    fail(&mut record, error.clone(), *remote);
                }
                record.identification.insert(key.clone(), outcome);
            }
            Ok(())
        })();
        if let Err(error) = operation {
            let remote = error
                .chain()
                .any(|cause| cause.is::<crate::diagnostics::RemoteError>());
            fail(&mut record, ErrorReport::capture(&error), remote);
        }
        record
    }

    fn publish(&self, record: &Record, file: &FileMatch, location: Location<'_>) -> Result<()> {
        let id = record
            .id
            .as_deref()
            .context("missing cache content ID")?
            .parse()?;
        match location {
            Location::Local => {
                let cache = self.cache.clone().open()?;
                cache.write_ident_info(id, file)?;
            }
            Location::Remote(session) => {
                remote::publish(session, self.cache.clone(), id, file.clone())?
            }
        }
        Ok(())
    }
}
fn fail(record: &mut Record, error: ErrorReport, remote: bool) {
    record.success = false;
    if record.error.is_none() {
        record.failed_stage = Some(Stage::Ident);
        record.error = Some(error);
        record.error_remote = remote;
    }
}

#[cfg(test)]
mod tests;
