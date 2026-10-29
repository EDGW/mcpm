//! Client-owned source enrichment after file-host import; providers and credentials never travel.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use mcpm_backend::data::cache::mods::ModsCacheSelection;
use mcpm_backend::data::runtime::RuntimeEnvironment;
use mcpm_backend::source::types::FileMatch;

use super::{
    Event, Record, Stage, Summary,
    args::{Meta, WriteArgs},
    record::IdentResult,
};
use crate::{
    commands::{execution::blocking, supports::mods::reuse},
    diagnostics::ErrorReport,
    flows::identify::{self as pipeline, Event as IdentifyEvent, Outcome, Target},
};

/// Captures the storage phase only when source enrichment is requested.
/// The input protocol is validated before paths or IDs are used for client requests.
pub(in crate::commands) struct Completion {
    enabled: bool,
    force: bool,
    parallelism: usize,
    specifications: Vec<String>,
    keys: Vec<String>,
    files: Vec<Record>,
    summary: Option<Summary>,
    validator: super::validation::Validator,
}
impl Completion {
    pub fn new(
        writing: &WriteArgs,
        specifications: &[String],
        session: &mcpm_backend::Session,
    ) -> Result<Self> {
        let enabled = writing.selected_metas()?.contains(&Meta::Ident);
        let keys: Vec<_> = if enabled {
            crate::commands::execution::Runtime::new()?
                .block_on(pipeline::profiles(session, specifications))?
                .into_iter()
                .map(|profile| profile.cache_key)
                .collect()
        } else {
            Vec::new()
        };
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
            force: writing.force(),
            parallelism: writing.parallelism(),
            specifications: specifications.to_vec(),
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
        session: &mcpm_backend::Session,
        environment: &RuntimeEnvironment,
        mut emit: impl FnMut(Event) -> Result<()>,
    ) -> Result<()> {
        self.validator.finish()?;
        if !self.enabled {
            return Ok(());
        }
        let selection = reuse::selection(environment);
        let target = Target {
            session: session.clone(),
            cache: if self.force { None } else { selection.clone() },
        };
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
        crate::commands::execution::Runtime::new()?.with_workers(|handle| {
            blocking::run(
                self.files.len(),
                self.parallelism,
                |index| {
                    self.process(
                        self.files[index].clone(),
                        &target,
                        selection.as_ref(),
                        handle,
                    )
                },
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
        target: &Target,
        selection: Option<&ModsCacheSelection>,
        runtime: &tokio::runtime::Handle,
    ) -> Record {
        let Some(path) = record.data_path.clone() else {
            return record;
        };
        let operation = (|| -> Result<()> {
            let mut outcomes = vec![None; self.keys.len()];
            let mut remote_failures = vec![false; self.keys.len()];
            runtime.block_on(pipeline::run(
                target.clone(),
                self.specifications.clone(),
                vec![path],
                1,
                |event| {
                    if let IdentifyEvent::Item {
                        source,
                        outcome,
                        remote,
                        ..
                    } = event
                    {
                        if let Some(slot) = outcomes.get_mut(source) {
                            *slot = Some(outcome);
                        }
                        if let Some(slot) = remote_failures.get_mut(source) {
                            *slot = remote;
                        }
                    }
                    Ok(())
                },
            ))?;
            for (source_index, key) in self.keys.iter().enumerate() {
                let outcome = outcomes[source_index]
                    .take()
                    .context("missing source identification result")?;
                let outcome = match outcome {
                    Outcome::Matched { cache, .. } => {
                        let publication = selection
                            .context("Mod cache is disabled")
                            .and_then(|selection| {
                                self.publish(&record, cache.as_ref(), target, selection, runtime)
                            });
                        match publication {
                            Ok(()) => IdentResult::Matched,
                            Err(error) => IdentResult::Failed {
                                error: ErrorReport::capture(&error),
                                remote: !target.is_local(),
                            },
                        }
                    }
                    Outcome::Unmatched => IdentResult::Unmatched,
                    Outcome::Failed { error } => IdentResult::Failed {
                        error,
                        remote: remote_failures[source_index],
                    },
                };
                if let IdentResult::Failed { error, remote } = &outcome {
                    fail(&mut record, error.clone(), *remote);
                }
                record.identification.insert(key.clone(), outcome);
            }
            Ok(())
        })();
        if let Err(error) = operation {
            let remote = kako_mc_lib::error::contains::<crate::diagnostics::RemoteError>(error.as_ref());
            fail(&mut record, ErrorReport::capture(&error), remote);
        }
        record
    }

    fn publish(
        &self,
        record: &Record,
        file: &FileMatch,
        target: &Target,
        selection: &ModsCacheSelection,
        runtime: &tokio::runtime::Handle,
    ) -> Result<()> {
        let id = record
            .id
            .as_deref()
            .context("missing cache content ID")?
            .to_owned();
        let remote = !target.is_local();
        let request = mcpm_backend::cache::IdentWriteInput {
            cache: selection.clone(),
            entries: vec![mcpm_backend::cache::IdentEntry {
                key: id,
                file: file.clone(),
            }],
            force: true,
            protect_stash: false,
        };
        runtime.block_on(async {
            let mut handle = target
                .session
                .start::<mcpm_backend::cache::IdentWrite>(request)
                .await
                .map_err(|failure| crate::commands::execution::failure(failure, remote))?;
            while let Some(_event) = handle.next().await {}
            handle
                .finish()
                .await
                .map_err(|failure| crate::commands::execution::failure(failure, remote))?;
            Ok::<(), anyhow::Error>(())
        })
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
