//! Client verification progress rendered on stderr with the configured theme.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use kako_mods_lib::local::mc::client::{CheckEvent, CheckProgress, CheckStep};

use super::super::output::text;
use crate::commands::output_style::OutputStyle;
use crate::terminal;

pub(super) struct BatchProgress {
    display: MultiProgress,
    overall: Option<ProgressBar>,
    enabled: bool,
    _logging: crate::logging::ProgressGuard,
}

impl BatchProgress {
    pub(super) fn new(total: usize) -> Self {
        let enabled = terminal::stderr_is_tty();
        let display = MultiProgress::with_draw_target(if enabled {
            ProgressDrawTarget::stderr_with_hz(10)
        } else {
            ProgressDrawTarget::hidden()
        });
        let overall = (total > 1).then(|| {
            let bar = display.add(ProgressBar::new(total as u64));
            let template = "{prefix} {pos}/{len} {wide_bar} {percent}% {msg}";
            bar.set_style(
                ProgressStyle::with_template(&crate::theme::progress_template(template))
                    .expect("batch check progress template must be valid"),
            );
            bar.set_prefix(OutputStyle::stderr().primary("Versions"));
            bar
        });
        let logging = crate::logging::attach_progress(enabled.then(|| display.clone()));
        Self {
            display,
            overall,
            enabled,
            _logging: logging,
        }
    }

    pub(super) fn start(&self, name: &str, root: &Path) -> Progress {
        if let Some(bar) = &self.overall {
            bar.set_message(OutputStyle::stderr().important(text(name)));
        }
        Progress::new(self, name, root)
    }

    pub(super) fn finish_version(&self) {
        if let Some(bar) = &self.overall {
            bar.inc(1);
        }
    }

    pub(super) fn suspend<T>(&self, output: impl FnOnce() -> T) -> T {
        self.display.suspend(output)
    }
}

impl Drop for BatchProgress {
    fn drop(&mut self) {
        if let Some(bar) = &self.overall {
            bar.finish_and_clear();
        }
        let _ = self.display.clear();
    }
}

pub(super) struct Progress {
    display: MultiProgress,
    main: ProgressBar,
    step: ProgressBar,
    root: PathBuf,
    name: String,
    last_draw: Option<Instant>,
    determinate: Option<bool>,
    enabled: bool,
}

impl Progress {
    fn new(batch: &BatchProgress, name: &str, root: &Path) -> Self {
        let enabled = batch.enabled;
        let display = batch.display.clone();
        let main = display.add(ProgressBar::new(1));
        let step = display.add(ProgressBar::new_spinner());
        let template = "{prefix}\n{wide_bar} {percent}%";
        main.set_style(
            ProgressStyle::with_template(&crate::theme::progress_template(template))
                .expect("main check progress template must be valid"),
        );
        step.enable_steady_tick(Duration::from_millis(100));
        Self {
            display,
            main,
            step,
            root: root.to_owned(),
            name: text(name),
            last_draw: None,
            determinate: None,
            enabled,
        }
    }

    pub(super) fn update(&mut self, progress: CheckProgress<'_>) {
        if !self.enabled {
            return;
        }
        let force = !matches!(progress.event, CheckEvent::FileFinished { .. });
        if !force
            && self
                .last_draw
                .is_some_and(|last| last.elapsed() < Duration::from_millis(100))
        {
            return;
        }
        self.last_draw = Some(Instant::now());
        let name = match progress.step {
            CheckStep::Preparing => "Preparing",
            CheckStep::VersionJar => "Checking version JAR",
            CheckStep::Libraries => "Checking libraries",
            CheckStep::Logging => "Checking logging config",
            CheckStep::AssetIndex => "Checking asset index",
            CheckStep::AssetObjects => "Checking asset objects",
            CheckStep::Finished => "Finished",
        };
        let style = OutputStyle::stderr();
        self.main.set_prefix(format!(
            "{} ({})",
            style.primary(format!(
                "Step {}/{} {name}",
                progress.step_number, progress.total_steps
            )),
            style.important(&self.name)
        ));
        self.main
            .set_length((progress.total_steps as u64).saturating_mul(1000).max(1));
        let fraction = if matches!(
            progress.event,
            CheckEvent::StepFinished | CheckEvent::Finished
        ) {
            0
        } else {
            progress
                .step_total_files
                .filter(|total| *total > 0)
                .map(|total| {
                    (progress.step_completed_files as u64).saturating_mul(1000) / total as u64
                })
                .unwrap_or(0)
        };
        self.main.set_position(
            (progress.completed_steps as u64)
                .saturating_mul(1000)
                .saturating_add(fraction),
        );
        let total = progress
            .step_total_files
            .map(|total| total.to_string())
            .unwrap_or_else(|| "?".into());
        self.step
            .set_prefix(style.important(format!("{}/{total}", progress.step_completed_files)));
        let path = progress
            .next_file
            .map(|file| file.path.as_path())
            .or_else(|| {
                (progress.step_completed_files > 0)
                    .then(|| progress.completed.last().map(|file| file.path.as_path()))
                    .flatten()
            });
        let filename = path
            .map(|path| {
                text(
                    &path
                        .strip_prefix(&self.root)
                        .unwrap_or(path)
                        .display()
                        .to_string(),
                )
            })
            .unwrap_or_else(|| "-".into());
        self.step.set_message(style.muted(format!("[{filename}]")));
        let determinate = progress.step_total_files.is_some();
        if self.determinate != Some(determinate) {
            let template = if determinate {
                "{prefix} {wide_msg}\n{wide_bar} {percent}%"
            } else {
                "{prefix} {wide_msg}\n{spinner}"
            };
            self.step.set_style(
                ProgressStyle::with_template(&crate::theme::progress_template(template))
                    .expect("step check progress template must be valid"),
            );
            self.determinate = Some(determinate);
        }
        if let Some(total) = progress.step_total_files {
            self.step.set_length((total as u64).max(1));
            self.step.set_position(if total == 0 {
                1
            } else {
                progress.step_completed_files as u64
            });
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.main.finish_and_clear();
        self.step.finish_and_clear();
        self.display.remove(&self.main);
        self.display.remove(&self.step);
    }
}
