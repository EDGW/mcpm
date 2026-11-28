//! Declarative and CEL-based conditional auto-restart policies for local Minecraft servers.

mod cel;
mod engine;
mod model;

pub use cel::{CelCompileError, CelEvalError};
pub use engine::{
    CompiledRestartPolicy, EvaluationOutcome, ExitObservation, ObservedCrashReport,
    RestartDecision, RestartHistory, current_local_timestamp,
};
pub use model::{
    BackoffRule, CustomRule, DEFAULT_BACKOFF_INITIAL_DELAY, DEFAULT_BACKOFF_MAX_DELAY,
    DEFAULT_BACKOFF_MULTIPLIER, DEFAULT_BACKOFF_RESET_AFTER, DEFAULT_RESTART_DELAY,
    DurationParseError, ExitKind, ExitMatcher, ExitMatchers, MAX_RESTART_DURATION, RestartAction,
    RestartMode, RestartPolicy, RestartRule, SimpleRule, parse_duration,
};

#[cfg(test)]
mod tests;
