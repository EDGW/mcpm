use std::path::PathBuf;

use super::{CheckReport, CheckTarget, CheckedFile, FileCheckStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStep {
    Preparing,
    VersionJar,
    Libraries,
    AssetIndex,
    AssetObjects,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckEvent {
    Started,
    StepStarted,
    PlanUpdated,
    FileFinished { status: FileCheckStatus },
    StepFinished,
    Finished,
}

/// One planned physical file. Invalid declarations are report issues, not files.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub step: CheckStep,
    pub target: CheckTarget,
    pub path: PathBuf,
    pub size: Option<u64>,
    pub mode: super::CheckOptionValue,
    pub(super) checksums: Vec<String>,
}

/// A synchronous borrowed snapshot, valid for the duration of the callback.
/// Completed includes failed/unverified attempts. Pending includes the next file
/// to check and only files discovered so far. Asset objects are discovered
/// after verifying/parsing the index, so overall totals can initially be unknown.
#[derive(Debug, Clone, Copy)]
pub struct CheckProgress<'a> {
    pub event: CheckEvent,
    pub step: CheckStep,
    pub completed: &'a [CheckedFile],
    pub pending: &'a [PlannedFile],
    /// The next file in the current step; no callbacks run while reading it.
    pub next_file: Option<&'a PlannedFile>,
    pub step_number: usize,
    pub total_steps: usize,
    pub completed_steps: usize,
    pub completed_files: usize,
    /// Pending count; None if inventory is incomplete.
    pub remaining_files: Option<usize>,
    pub total_files: Option<usize>,
    pub step_completed_files: usize,
    pub step_total_files: Option<usize>,
}

pub(super) struct Progress<'a> {
    observer: &'a mut dyn FnMut(CheckProgress<'_>),
    plan: Vec<PlannedFile>,
    cursor: usize,
    step: CheckStep,
    step_active: bool,
    step_totals: [usize; 6],
    step_done: [usize; 6],
    inventory_complete: bool,
    completed_steps: usize,
    total_steps: usize,
}

impl<'a> Progress<'a> {
    pub(super) fn new(
        observer: &'a mut dyn FnMut(CheckProgress<'_>),
        report: &CheckReport,
    ) -> Self {
        let mut progress = Self {
            observer,
            plan: Vec::new(),
            cursor: 0,
            step: CheckStep::Preparing,
            step_active: true,
            step_totals: [0; 6],
            step_done: [0; 6],
            inventory_complete: false,
            completed_steps: 0,
            total_steps: 1
                + usize::from(report.options.version_jar.is_enabled())
                + usize::from(report.options.libraries.is_enabled())
                + usize::from(
                    report.options.asset_index.is_enabled() || report.options.assets.is_enabled(),
                )
                + usize::from(report.options.assets.is_enabled()),
        };
        progress.emit(CheckEvent::Started, report);
        progress.emit(CheckEvent::StepStarted, report);
        progress
    }

    fn emit(&mut self, event: CheckEvent, report: &CheckReport) {
        let pending = &self.plan[self.cursor..];
        let next_file = pending.first().filter(|file| file.step == self.step);
        (self.observer)(CheckProgress {
            event,
            step: self.step,
            completed: &report.files,
            pending,
            next_file,
            step_number: if self.step == CheckStep::Finished {
                self.completed_steps
            } else {
                self.completed_steps + usize::from(self.step_active)
            },
            total_steps: self.total_steps,
            completed_steps: self.completed_steps,
            completed_files: report.files.len(),
            remaining_files: self.inventory_complete.then_some(pending.len()),
            total_files: self.inventory_complete.then_some(self.plan.len()),
            step_completed_files: self.step_done[self.step as usize],
            step_total_files: (!matches!(self.step, CheckStep::Preparing | CheckStep::Finished))
                .then_some(self.step_totals[self.step as usize]),
        });
    }

    pub(super) fn add_plan(
        &mut self,
        files: Vec<PlannedFile>,
        inventory_complete: bool,
        report: &CheckReport,
    ) {
        for file in &files {
            self.step_totals[file.step as usize] += 1;
        }
        self.plan.extend(files);
        self.inventory_complete = inventory_complete;
        self.emit(CheckEvent::PlanUpdated, report);
    }

    pub(super) fn start_step(&mut self, step: CheckStep, report: &CheckReport) {
        self.end_step(report);
        self.step = step;
        self.step_active = true;
        self.emit(CheckEvent::StepStarted, report);
    }

    pub(super) fn end_step(&mut self, report: &CheckReport) {
        if self.step_active {
            self.completed_steps += 1;
            self.step_active = false;
            self.emit(CheckEvent::StepFinished, report);
        }
    }

    pub(super) fn next_file(&self) -> Option<PlannedFile> {
        self.plan
            .get(self.cursor)
            .filter(|file| file.step == self.step)
            .cloned()
    }

    pub(super) fn complete_file(&mut self, status: FileCheckStatus, report: &CheckReport) {
        self.cursor += 1;
        self.step_done[self.step as usize] += 1;
        self.emit(CheckEvent::FileFinished { status }, report);
    }

    pub(super) fn finish(&mut self, report: &CheckReport) {
        self.end_step(report);
        self.step = CheckStep::Finished;
        self.emit(CheckEvent::Finished, report);
    }
}
