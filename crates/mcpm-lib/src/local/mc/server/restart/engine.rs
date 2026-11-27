//! Pure synchronous evaluation engine and history tracker for auto-restart policies.

use crate::local::mc::{
    crash_report::{CrashReportAnalysis, CrashReportType},
    server::launch::LaunchExit,
};
use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset};
use std::time::{Duration, Instant};

use super::{
    cel::{self, CelContextInput, CompiledExpression},
    model::{
        BackoffRule, CustomRule, ExitKind, ExitMatchers, RestartAction, RestartMode,
        RestartPolicy, SimpleRule,
    },
};

const MAX_HISTORY_RECORDS: usize = 4096;

/// Pre-compiled restart policy ready for repeated evaluation across server generations.
#[derive(Debug)]
pub struct CompiledRestartPolicy {
    rules: Vec<CompiledRule>,
}

impl CompiledRestartPolicy {
    /// Validates and pre-compiles all rules, including CEL expressions and duration strings.
    pub fn compile(policy: &RestartPolicy) -> Result<Self> {
        policy.validate()?;
        let mut rules = Vec::with_capacity(policy.rules.len());
        for (index, rule) in policy.rules.iter().enumerate() {
            let name = rule.display_name(index);
            let mode = match &rule.mode {
                RestartMode::Simple(simple) => CompiledMode::Simple(compile_simple(simple)?),
                RestartMode::Backoff(backoff) => CompiledMode::Backoff(compile_backoff(backoff)?),
                RestartMode::Custom(custom) => CompiledMode::Custom(compile_custom(custom)?),
            };
            rules.push(CompiledRule {
                index,
                name,
                action: rule.action,
                mode,
            });
        }
        Ok(Self { rules })
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Evaluates the policy against a completed Java run, mutating `history` with the observed
    /// exit and any chosen restart decision.
    pub fn evaluate(
        &self,
        observation: &ExitObservation,
        history: &mut RestartHistory,
    ) -> EvaluationOutcome {
        let exit_kind = observation.exit_kind();
        let crash_type = observation.crash_type();
        let exit_code = observation.exit.shell_code();

        history.record_exit(ExitRecord {
            kind: exit_kind,
            crash_type,
            at: observation.at,
        });
        if exit_kind == ExitKind::Crash {
            history.consecutive_crashes = history.consecutive_crashes.saturating_add(1);
        } else {
            history.consecutive_crashes = 0;
        }

        // Reset any backoff rule whose uptime threshold was satisfied by this run.
        for rule in &self.rules {
            if let CompiledMode::Backoff(backoff) = &rule.mode
                && observation.uptime >= backoff.reset_after
            {
                history.reset_backoff_streak(rule.index);
            }
        }

        if self.rules.is_empty() {
            return EvaluationOutcome {
                decision: RestartDecision::Stop { matched_rule: None },
                diagnostics: Vec::new(),
            };
        }

        let mut diagnostics = Vec::new();
        let mut cel_context = None;
        let has_custom = self
            .rules
            .iter()
            .any(|rule| matches!(rule.mode, CompiledMode::Custom(_)));
        if has_custom {
            let input = observation.to_cel_input(history);
            match cel::build_context(&input) {
                Ok(ctx) => cel_context = Some(ctx),
                Err(error) => {
                    diagnostics.push(format!("failed to initialize CEL context: {error}"));
                }
            }
        }

        for rule in &self.rules {
            let delay = match &rule.mode {
                CompiledMode::Simple(simple) => {
                    if !matches_exit(&simple.exit_type, &simple.exit_codes, exit_kind, crash_type, exit_code) {
                        continue;
                    }
                    if let Some(max_runs) = simple.max_runs {
                        let used = history.count_executions_by_rule(
                            rule.index,
                            observation.at,
                            simple.window,
                        );
                        if used >= max_runs as usize {
                            continue;
                        }
                    }
                    simple.delay
                }
                CompiledMode::Backoff(backoff) => {
                    if !matches_exit(&backoff.exit_type, &backoff.exit_codes, exit_kind, crash_type, exit_code) {
                        continue;
                    }
                    let streak = history.backoff_streak(rule.index);
                    if backoff.max_consecutive.is_some_and(|max| streak >= max) {
                        continue;
                    }
                    compute_backoff_delay(backoff, streak)
                }
                CompiledMode::Custom(custom) => {
                    let Some(ctx) = cel_context.as_ref() else {
                        continue;
                    };
                    match custom.expression.evaluate(ctx) {
                        Ok(true) => custom.delay,
                        Ok(false) => continue,
                        Err(error) => {
                            diagnostics.push(format!(
                                "restart_on rule {:?} evaluation error: {error}",
                                rule.name
                            ));
                            continue;
                        }
                    }
                }
            };

            // First matching rule decides the outcome.
            return match rule.action {
                RestartAction::Stop => EvaluationOutcome {
                    decision: RestartDecision::Stop {
                        matched_rule: Some(rule.name.clone()),
                    },
                    diagnostics,
                },
                RestartAction::Restart => {
                    if matches!(rule.mode, CompiledMode::Backoff(_)) {
                        history.increment_backoff_streak(rule.index);
                    }
                    history.record_execution(ExecutionRecord {
                        rule_index: rule.index,
                        rule_name: rule.name.clone(),
                        at: observation.at,
                    });
                    history.total_restarts = history.total_restarts.saturating_add(1);
                    let next_attempt = history.total_restarts.saturating_add(1);
                    EvaluationOutcome {
                        decision: RestartDecision::Restart {
                            rule: rule.name.clone(),
                            delay,
                            attempt: next_attempt,
                        },
                        diagnostics,
                    }
                }
            };
        }

        EvaluationOutcome {
            decision: RestartDecision::Stop { matched_rule: None },
            diagnostics,
        }
    }
}

/// Returns the current Host wall-clock time with its local UTC offset for CEL `now`.
pub fn current_local_timestamp() -> DateTime<FixedOffset> {
    chrono::Local::now().fixed_offset()
}

/// Pure `mcpm-lib` description of a detected crash report for restart evaluation.
#[derive(Debug, Clone)]
pub struct ObservedCrashReport {
    pub path: String,
    pub analysis: Result<CrashReportAnalysis, String>,
}

/// Pure `mcpm-lib` facts about a single completed Java process generation.
#[derive(Debug, Clone)]
pub struct ExitObservation {
    pub exit: LaunchExit,
    pub killed_by_mcpm: bool,
    pub stop_requested: bool,
    pub crash_report: Option<ObservedCrashReport>,
    pub uptime: Duration,
    pub rcon_ever_connected: bool,
    pub now: DateTime<FixedOffset>,
    pub at: Instant,
}

impl ExitObservation {
    pub fn exit_kind(&self) -> ExitKind {
        ExitKind::classify(
            self.exit,
            self.killed_by_mcpm,
            self.crash_report.is_some(),
        )
    }

    pub fn crash_type(&self) -> Option<CrashReportType> {
        self.crash_report
            .as_ref()
            .and_then(|report| report.analysis.as_ref().ok())
            .map(|analysis| analysis.report_type)
    }

    fn to_cel_input<'a>(&'a self, history: &'a RestartHistory) -> CelContextInput<'a> {
        let exit_signal = match self.exit {
            LaunchExit::Signal(sig) => i64::from(sig),
            LaunchExit::Code(_) => 0,
        };
        let (crash_detected, crash_parsed, crash_type, crash_desc, crash_path, crash_error) =
            match &self.crash_report {
                None => (false, false, None, "", "", ""),
                Some(report) => match &report.analysis {
                    Ok(analysis) => (
                        true,
                        true,
                        Some(analysis.report_type),
                        analysis.desc.as_deref().unwrap_or(""),
                        report.path.as_str(),
                        "",
                    ),
                    Err(error) => (
                        true,
                        false,
                        None,
                        "",
                        report.path.as_str(),
                        error.as_str(),
                    ),
                },
            };

        CelContextInput {
            exit_kind: self.exit_kind(),
            exit_code: i64::from(self.exit.shell_code()),
            exit_signal,
            stop_requested: self.stop_requested,
            crash_detected,
            crash_parsed,
            crash_type,
            crash_desc,
            crash_path,
            crash_error,
            uptime: self.uptime,
            attempt: history.current_attempt(),
            total_restarts: history.total_restarts,
            consecutive_crashes: history.consecutive_crashes,
            rcon_ever_connected: self.rcon_ever_connected,
            now: self.now,
            now_instant: self.at,
            executions: &history.executions,
            exits: &history.exits,
        }
    }
}

/// Outcome of evaluating a [`CompiledRestartPolicy`] after a Java process exits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationOutcome {
    pub decision: RestartDecision,
    pub diagnostics: Vec<String>,
}

/// Decision produced by evaluating `[[restart_on]]` rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartDecision {
    Restart {
        rule: String,
        delay: Duration,
        /// 1-based generation number of the upcoming process (`2` for the first restart).
        attempt: u32,
    },
    Stop {
        /// Rule name if an explicit `action = "stop"` rule matched; `None` if no rule matched.
        matched_rule: Option<String>,
    },
}

/// Bounded in-memory restart history owned by the shim supervision loop.
#[derive(Debug, Default, Clone)]
pub struct RestartHistory {
    executions: Vec<ExecutionRecord>,
    exits: Vec<ExitRecord>,
    backoff_streaks: Vec<(usize, u32)>,
    total_restarts: u32,
    consecutive_crashes: u32,
}

impl RestartHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 1-based generation number of the generation that just ran.
    pub fn current_attempt(&self) -> u32 {
        self.total_restarts.saturating_add(1)
    }

    pub fn total_restarts(&self) -> u32 {
        self.total_restarts
    }

    pub fn consecutive_crashes(&self) -> u32 {
        self.consecutive_crashes
    }

    fn record_exit(&mut self, record: ExitRecord) {
        if self.exits.len() >= MAX_HISTORY_RECORDS {
            self.exits.remove(0);
        }
        self.exits.push(record);
    }

    fn record_execution(&mut self, record: ExecutionRecord) {
        if self.executions.len() >= MAX_HISTORY_RECORDS {
            self.executions.remove(0);
        }
        self.executions.push(record);
    }

    fn count_executions_by_rule(
        &self,
        rule_index: usize,
        now: Instant,
        window: Option<Duration>,
    ) -> usize {
        self.executions
            .iter()
            .filter(|record| {
                record.rule_index == rule_index && within_window(record.at, now, window)
            })
            .count()
    }

    fn backoff_streak(&self, rule_index: usize) -> u32 {
        self.backoff_streaks
            .iter()
            .find(|(idx, _)| *idx == rule_index)
            .map(|(_, streak)| *streak)
            .unwrap_or(0)
    }

    fn increment_backoff_streak(&mut self, rule_index: usize) {
        if let Some((_, streak)) = self
            .backoff_streaks
            .iter_mut()
            .find(|(idx, _)| *idx == rule_index)
        {
            *streak = streak.saturating_add(1);
        } else {
            self.backoff_streaks.push((rule_index, 1));
        }
    }

    fn reset_backoff_streak(&mut self, rule_index: usize) {
        if let Some((_, streak)) = self
            .backoff_streaks
            .iter_mut()
            .find(|(idx, _)| *idx == rule_index)
        {
            *streak = 0;
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ExecutionRecord {
    pub rule_index: usize,
    pub rule_name: String,
    pub at: Instant,
}

#[derive(Debug, Clone)]
pub(super) struct ExitRecord {
    pub kind: ExitKind,
    pub crash_type: Option<CrashReportType>,
    pub at: Instant,
}

pub(super) fn within_window(at: Instant, now: Instant, window: Option<Duration>) -> bool {
    match window {
        None => true,
        Some(window) => now.saturating_duration_since(at) <= window,
    }
}

#[derive(Debug)]
struct CompiledRule {
    index: usize,
    name: String,
    action: RestartAction,
    mode: CompiledMode,
}

#[derive(Debug)]
enum CompiledMode {
    Simple(CompiledSimple),
    Backoff(CompiledBackoff),
    Custom(CompiledCustom),
}

#[derive(Debug)]
struct CompiledSimple {
    exit_type: ExitMatchers,
    exit_codes: Vec<u32>,
    max_runs: Option<u32>,
    window: Option<Duration>,
    delay: Duration,
}

#[derive(Debug)]
struct CompiledBackoff {
    exit_type: ExitMatchers,
    exit_codes: Vec<u32>,
    initial_delay: Duration,
    max_delay: Duration,
    multiplier: f64,
    reset_after: Duration,
    max_consecutive: Option<u32>,
}

#[derive(Debug)]
struct CompiledCustom {
    expression: CompiledExpression,
    delay: Duration,
}

fn compile_simple(rule: &SimpleRule) -> Result<CompiledSimple> {
    Ok(CompiledSimple {
        exit_type: rule.exit_type.clone(),
        exit_codes: rule.exit_codes.clone(),
        max_runs: rule.max_runs,
        window: rule.parsed_window().context("invalid window")?,
        delay: rule.parsed_delay().context("invalid delay")?,
    })
}

fn compile_backoff(rule: &BackoffRule) -> Result<CompiledBackoff> {
    Ok(CompiledBackoff {
        exit_type: rule.exit_type.clone(),
        exit_codes: rule.exit_codes.clone(),
        initial_delay: rule
            .parsed_initial_delay()
            .context("invalid initial_delay")?,
        max_delay: rule.parsed_max_delay().context("invalid max_delay")?,
        multiplier: rule.effective_multiplier(),
        reset_after: rule.parsed_reset_after().context("invalid reset_after")?,
        max_consecutive: rule.max_consecutive,
    })
}

fn compile_custom(rule: &CustomRule) -> Result<CompiledCustom> {
    Ok(CompiledCustom {
        expression: cel::compile(&rule.condition)?,
        delay: rule.parsed_delay().context("invalid delay")?,
    })
}

fn matches_exit(
    matchers: &ExitMatchers,
    exit_codes: &[u32],
    exit_kind: ExitKind,
    crash_type: Option<CrashReportType>,
    exit_code: u32,
) -> bool {
    if !matchers.matches(exit_kind, crash_type) {
        return false;
    }
    exit_codes.is_empty() || exit_codes.contains(&exit_code)
}

fn compute_backoff_delay(rule: &CompiledBackoff, streak: u32) -> Duration {
    if streak == 0 || rule.initial_delay.is_zero() {
        return rule.initial_delay.min(rule.max_delay);
    }
    let exponent = i32::try_from(streak).unwrap_or(i32::MAX);
    let factor = rule.multiplier.powi(exponent);
    if !factor.is_finite() {
        return rule.max_delay;
    }
    let scaled_secs = rule.initial_delay.as_secs_f64() * factor;
    let max_secs = rule.max_delay.as_secs_f64();
    if !scaled_secs.is_finite() || scaled_secs >= max_secs {
        return rule.max_delay;
    }
    Duration::from_secs_f64(scaled_secs.max(0.0)).min(rule.max_delay)
}
