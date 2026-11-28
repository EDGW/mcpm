//! Configuration schema, duration parsing, and static validation for server auto-restart policies.

use crate::local::mc::{crash_report::CrashReportType, server::launch::LaunchExit};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeSet, time::Duration};
use thiserror::Error;

use super::cel;

/// Default delay before respawning Java for `simple` and `custom` rules.
pub const DEFAULT_RESTART_DELAY: Duration = Duration::from_secs(5);
/// Default initial delay for `backoff` rules.
pub const DEFAULT_BACKOFF_INITIAL_DELAY: Duration = Duration::from_secs(5);
/// Default maximum delay cap for `backoff` rules.
pub const DEFAULT_BACKOFF_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
/// Default backoff multiplier.
pub const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;
/// Default uptime threshold that resets a `backoff` rule's consecutive crash streak.
pub const DEFAULT_BACKOFF_RESET_AFTER: Duration = Duration::from_secs(10 * 60);

/// Maximum accepted duration for restart delays, windows, and reset thresholds (365 days).
pub const MAX_RESTART_DURATION: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// Ordered list of auto-restart rules configured via `[[restart_on]]` in `conf.toml`.
/// Rules are evaluated top-to-bottom on each Java exit; the first matching rule decides the action.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RestartPolicy {
    pub rules: Vec<RestartRule>,
}

impl RestartPolicy {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Validates all rules, rule name uniqueness, duration syntax, numeric bounds, and CEL syntax.
    pub fn validate(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        for (index, rule) in self.rules.iter().enumerate() {
            let label = rule.display_name(index);
            if let Some(name) = &rule.name {
                ensure!(
                    !name.trim().is_empty() && !name.contains('\0'),
                    "restart_on[{index}].name must be nonempty and contain no NUL"
                );
            }
            ensure!(
                names.insert(label.clone()),
                "duplicate restart_on rule name {label:?}"
            );
            rule.validate()
                .with_context(|| format!("invalid restart_on rule {label}"))?;
        }
        Ok(())
    }
}

/// A single `[[restart_on]]` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct RestartRule {
    pub name: Option<String>,
    pub action: RestartAction,
    pub mode: RestartMode,
}

impl RestartRule {
    /// Returns the explicit rule name or a stable 1-based fallback label (`#1`, `#2`, ...).
    pub fn display_name(&self, index: usize) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("#{}", index + 1))
    }

    pub fn validate(&self) -> Result<()> {
        match &self.mode {
            RestartMode::Simple(rule) => rule.validate(),
            RestartMode::Backoff(rule) => rule.validate(),
            RestartMode::Custom(rule) => rule.validate(),
        }
    }
}

/// Action taken when a rule matches.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartAction {
    #[default]
    Restart,
    Stop,
}

impl RestartAction {
    fn is_default(&self) -> bool {
        matches!(self, Self::Restart)
    }
}

/// Strategy-specific matching and delay parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum RestartMode {
    Simple(SimpleRule),
    Backoff(BackoffRule),
    Custom(CustomRule),
}

/// Fixed-delay restart rule with optional exit-type/code filter and sliding-window budget.
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleRule {
    pub exit_type: ExitMatchers,
    pub exit_codes: Vec<u32>,
    pub max_runs: Option<u32>,
    pub window: Option<String>,
    pub delay: Option<String>,
}

impl SimpleRule {
    pub fn parsed_window(&self) -> Result<Option<Duration>, DurationParseError> {
        self.window.as_deref().map(parse_duration).transpose()
    }

    pub fn parsed_delay(&self) -> Result<Duration, DurationParseError> {
        match self.delay.as_deref() {
            Some(value) => parse_duration(value),
            None => Ok(DEFAULT_RESTART_DELAY),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.exit_type.0.is_empty(),
            "exit_type must contain at least one matcher"
        );
        if let Some(max_runs) = self.max_runs {
            ensure!(max_runs >= 1, "max_runs must be at least 1");
        }
        if let Some(window) = self.parsed_window()? {
            ensure!(
                self.max_runs.is_some(),
                "window requires max_runs in simple mode"
            );
            ensure!(!window.is_zero(), "window must be greater than zero");
        }
        let _ = self.parsed_delay()?;
        Ok(())
    }
}

/// Exponential backoff rule designed to absorb crash loops without busy-restarting.
#[derive(Debug, Clone, PartialEq)]
pub struct BackoffRule {
    pub exit_type: ExitMatchers,
    pub exit_codes: Vec<u32>,
    pub initial_delay: Option<String>,
    pub max_delay: Option<String>,
    pub multiplier: Option<f64>,
    pub reset_after: Option<String>,
    pub max_consecutive: Option<u32>,
}

impl BackoffRule {
    pub fn parsed_initial_delay(&self) -> Result<Duration, DurationParseError> {
        match self.initial_delay.as_deref() {
            Some(value) => parse_duration(value),
            None => Ok(DEFAULT_BACKOFF_INITIAL_DELAY),
        }
    }

    pub fn parsed_max_delay(&self) -> Result<Duration, DurationParseError> {
        match self.max_delay.as_deref() {
            Some(value) => parse_duration(value),
            None => Ok(DEFAULT_BACKOFF_MAX_DELAY),
        }
    }

    pub fn effective_multiplier(&self) -> f64 {
        self.multiplier.unwrap_or(DEFAULT_BACKOFF_MULTIPLIER)
    }

    pub fn parsed_reset_after(&self) -> Result<Duration, DurationParseError> {
        match self.reset_after.as_deref() {
            Some(value) => parse_duration(value),
            None => Ok(DEFAULT_BACKOFF_RESET_AFTER),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.exit_type.0.is_empty(),
            "exit_type must contain at least one matcher"
        );
        let initial = self.parsed_initial_delay()?;
        let max = self.parsed_max_delay()?;
        ensure!(
            initial <= max,
            "initial_delay ({initial:?}) must not exceed max_delay ({max:?})"
        );
        let multiplier = self.effective_multiplier();
        ensure!(
            multiplier.is_finite() && multiplier >= 1.0,
            "multiplier must be finite and at least 1.0"
        );
        let reset_after = self.parsed_reset_after()?;
        ensure!(
            !reset_after.is_zero(),
            "reset_after must be greater than zero"
        );
        if let Some(max_consecutive) = self.max_consecutive {
            ensure!(max_consecutive >= 1, "max_consecutive must be at least 1");
        }
        Ok(())
    }
}

/// Custom CEL boolean expression rule.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomRule {
    pub condition: String,
    pub delay: Option<String>,
}

impl CustomRule {
    pub fn parsed_delay(&self) -> Result<Duration, DurationParseError> {
        match self.delay.as_deref() {
            Some(value) => parse_duration(value),
            None => Ok(DEFAULT_RESTART_DELAY),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.condition.trim().is_empty(),
            "condition must not be empty"
        );
        cel::compile(&self.condition)?;
        let _ = self.parsed_delay()?;
        Ok(())
    }
}

/// High-level exit classification shared by built-in matchers and CEL `exit.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Exit,
    Crash,
    KilledByMcpm,
    KilledByUser,
}

impl ExitKind {
    /// POSIX `SIGKILL` signal number, invariant across Unix hosts and cross-OS clients.
    pub const SIGKILL: i32 = 9;

    /// Single authoritative classification shared by restart evaluation and shim exit reporting.
    pub fn classify(exit: LaunchExit, killed_by_mcpm: bool, has_crash_report: bool) -> Self {
        match exit {
            LaunchExit::Code(0) if has_crash_report => Self::Crash,
            LaunchExit::Code(0) => Self::Exit,
            LaunchExit::Signal(Self::SIGKILL) if killed_by_mcpm => Self::KilledByMcpm,
            LaunchExit::Signal(Self::SIGKILL) => Self::KilledByUser,
            LaunchExit::Code(_) if killed_by_mcpm => Self::KilledByMcpm,
            LaunchExit::Code(_) | LaunchExit::Signal(_) => Self::Crash,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exit => "exit",
            Self::Crash => "crash",
            Self::KilledByMcpm => "killed_by_mcpm",
            Self::KilledByUser => "killed_by_user",
        }
    }
}

/// Symbolic exit matcher accepted by `exit_type` and CEL `exits([...], window)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitMatcher {
    Crash,
    Watchdog,
    OtherCrash,
    Exit,
    KilledByUser,
    KilledByMcpm,
    Any,
}

impl ExitMatcher {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "crash" => Some(Self::Crash),
            "watchdog" => Some(Self::Watchdog),
            "other_crash" => Some(Self::OtherCrash),
            "exit" => Some(Self::Exit),
            "killed_by_user" => Some(Self::KilledByUser),
            "killed_by_mcpm" => Some(Self::KilledByMcpm),
            "any" => Some(Self::Any),
            _ => None,
        }
    }

    pub fn matches(self, kind: ExitKind, crash_type: Option<CrashReportType>) -> bool {
        match self {
            Self::Any => true,
            Self::Crash => kind == ExitKind::Crash,
            Self::Watchdog => {
                kind == ExitKind::Crash && crash_type == Some(CrashReportType::Watchdog)
            }
            Self::OtherCrash => {
                kind == ExitKind::Crash && crash_type != Some(CrashReportType::Watchdog)
            }
            Self::Exit => kind == ExitKind::Exit,
            Self::KilledByUser => kind == ExitKind::KilledByUser,
            Self::KilledByMcpm => kind == ExitKind::KilledByMcpm,
        }
    }
}

/// One or more [`ExitMatcher`] entries; accepts either a single string or a list in TOML/JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitMatchers(pub Vec<ExitMatcher>);

impl Default for ExitMatchers {
    fn default() -> Self {
        Self(vec![ExitMatcher::Crash])
    }
}

impl ExitMatchers {
    pub fn matches(&self, kind: ExitKind, crash_type: Option<CrashReportType>) -> bool {
        self.0
            .iter()
            .any(|matcher| matcher.matches(kind, crash_type))
    }
}

impl Serialize for ExitMatchers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.len() == 1 {
            self.0[0].serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for ExitMatchers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany {
            One(ExitMatcher),
            Many(Vec<ExitMatcher>),
        }
        match OneOrMany::deserialize(deserializer)? {
            OneOrMany::One(matcher) => Ok(Self(vec![matcher])),
            OneOrMany::Many(matchers) => Ok(Self(matchers)),
        }
    }
}

/// Parses a duration string composed of integer segments with units `ms`, `s`, `m`, `h`, or `d`
/// (for example `"500ms"`, `"5s"`, `"10m"`, `"1h30m"`).
pub fn parse_duration(input: &str) -> Result<Duration, DurationParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(DurationParseError::Empty);
    }

    let bytes = trimmed.as_bytes();
    let mut index = 0;
    let mut total_ms: u128 = 0;

    while index < bytes.len() {
        let num_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == num_start {
            return Err(DurationParseError::InvalidSyntax {
                input: input.to_owned(),
            });
        }
        let value: u128 = trimmed[num_start..index]
            .parse()
            .map_err(|_| DurationParseError::Overflow {
                input: input.to_owned(),
            })?;

        let unit_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let unit = &trimmed[unit_start..index];
        let unit_ms: u128 = match unit {
            "ms" => 1,
            "s" => 1_000,
            "m" => 60 * 1_000,
            "h" => 60 * 60 * 1_000,
            "d" => 24 * 60 * 60 * 1_000,
            _ => {
                return Err(DurationParseError::UnknownUnit {
                    input: input.to_owned(),
                    unit: unit.to_owned(),
                });
            }
        };

        let segment_ms = value
            .checked_mul(unit_ms)
            .ok_or_else(|| DurationParseError::Overflow {
                input: input.to_owned(),
            })?;
        total_ms =
            total_ms
                .checked_add(segment_ms)
                .ok_or_else(|| DurationParseError::Overflow {
                    input: input.to_owned(),
                })?;
    }

    let secs = u64::try_from(total_ms / 1_000).map_err(|_| DurationParseError::Overflow {
        input: input.to_owned(),
    })?;
    let millis = (total_ms % 1_000) as u32;
    let duration = Duration::new(secs, millis * 1_000_000);
    if duration > MAX_RESTART_DURATION {
        return Err(DurationParseError::Overflow {
            input: input.to_owned(),
        });
    }
    Ok(duration)
}

#[derive(Debug, Error)]
pub enum DurationParseError {
    #[error("duration string must not be empty")]
    Empty,
    #[error("invalid duration {input:?}; expected segments like \"5s\", \"10m\", or \"1h30m\"")]
    InvalidSyntax { input: String },
    #[error(
        "unknown duration unit {unit:?} in {input:?}; supported units are ms, s, m, h, d"
    )]
    UnknownUnit { input: String, unit: String },
    #[error("duration {input:?} is too large")]
    Overflow { input: String },
}

// Serde bridge keeping `type` optional (defaulting to "simple") while rejecting unknown
// fields and mode-mismatched fields in both TOML and JSON.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RuleKind {
    #[default]
    Simple,
    Backoff,
    Custom,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartRuleWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "RestartAction::is_default")]
    action: RestartAction,
    #[serde(default, rename = "type")]
    kind: RuleKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_type: Option<ExitMatchers>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_codes: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_runs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial_delay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_delay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    multiplier: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reset_after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_consecutive: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    condition: Option<String>,
}

impl RestartRuleWire {
    fn into_rule(self) -> Result<RestartRule> {
        let mode = match self.kind {
            RuleKind::Simple => {
                reject_field("simple", "initial_delay", self.initial_delay.is_some())?;
                reject_field("simple", "max_delay", self.max_delay.is_some())?;
                reject_field("simple", "multiplier", self.multiplier.is_some())?;
                reject_field("simple", "reset_after", self.reset_after.is_some())?;
                reject_field("simple", "max_consecutive", self.max_consecutive.is_some())?;
                reject_field("simple", "condition", self.condition.is_some())?;
                RestartMode::Simple(SimpleRule {
                    exit_type: self.exit_type.unwrap_or_default(),
                    exit_codes: self.exit_codes.unwrap_or_default(),
                    max_runs: self.max_runs,
                    window: self.window,
                    delay: self.delay,
                })
            }
            RuleKind::Backoff => {
                reject_field("backoff", "max_runs", self.max_runs.is_some())?;
                reject_field("backoff", "window", self.window.is_some())?;
                reject_field("backoff", "delay", self.delay.is_some())?;
                reject_field("backoff", "condition", self.condition.is_some())?;
                RestartMode::Backoff(BackoffRule {
                    exit_type: self.exit_type.unwrap_or_default(),
                    exit_codes: self.exit_codes.unwrap_or_default(),
                    initial_delay: self.initial_delay,
                    max_delay: self.max_delay,
                    multiplier: self.multiplier,
                    reset_after: self.reset_after,
                    max_consecutive: self.max_consecutive,
                })
            }
            RuleKind::Custom => {
                reject_field("custom", "exit_type", self.exit_type.is_some())?;
                reject_field("custom", "exit_codes", self.exit_codes.is_some())?;
                reject_field("custom", "max_runs", self.max_runs.is_some())?;
                reject_field("custom", "window", self.window.is_some())?;
                reject_field("custom", "initial_delay", self.initial_delay.is_some())?;
                reject_field("custom", "max_delay", self.max_delay.is_some())?;
                reject_field("custom", "multiplier", self.multiplier.is_some())?;
                reject_field("custom", "reset_after", self.reset_after.is_some())?;
                reject_field("custom", "max_consecutive", self.max_consecutive.is_some())?;
                let Some(condition) = self.condition else {
                    bail!("missing required field 'condition' for type = \"custom\"");
                };
                RestartMode::Custom(CustomRule {
                    condition,
                    delay: self.delay,
                })
            }
        };
        Ok(RestartRule {
            name: self.name,
            action: self.action,
            mode,
        })
    }

    fn from_rule(rule: &RestartRule) -> Self {
        let mut wire = Self {
            name: rule.name.clone(),
            action: rule.action,
            kind: RuleKind::Simple,
            exit_type: None,
            exit_codes: None,
            max_runs: None,
            window: None,
            delay: None,
            initial_delay: None,
            max_delay: None,
            multiplier: None,
            reset_after: None,
            max_consecutive: None,
            condition: None,
        };
        match &rule.mode {
            RestartMode::Simple(simple) => {
                wire.kind = RuleKind::Simple;
                wire.exit_type = Some(simple.exit_type.clone());
                if !simple.exit_codes.is_empty() {
                    wire.exit_codes = Some(simple.exit_codes.clone());
                }
                wire.max_runs = simple.max_runs;
                wire.window = simple.window.clone();
                wire.delay = simple.delay.clone();
            }
            RestartMode::Backoff(backoff) => {
                wire.kind = RuleKind::Backoff;
                wire.exit_type = Some(backoff.exit_type.clone());
                if !backoff.exit_codes.is_empty() {
                    wire.exit_codes = Some(backoff.exit_codes.clone());
                }
                wire.initial_delay = backoff.initial_delay.clone();
                wire.max_delay = backoff.max_delay.clone();
                wire.multiplier = backoff.multiplier;
                wire.reset_after = backoff.reset_after.clone();
                wire.max_consecutive = backoff.max_consecutive;
            }
            RestartMode::Custom(custom) => {
                wire.kind = RuleKind::Custom;
                wire.condition = Some(custom.condition.clone());
                wire.delay = custom.delay.clone();
            }
        }
        wire
    }
}

fn reject_field(mode: &str, field: &str, present: bool) -> Result<()> {
    ensure!(
        !present,
        "field {field:?} is not supported when type = {mode:?}"
    );
    Ok(())
}

impl Serialize for RestartRule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RestartRuleWire::from_rule(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RestartRule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RestartRuleWire::deserialize(deserializer)?;
        wire.into_rule().map_err(serde::de::Error::custom)
    }
}
