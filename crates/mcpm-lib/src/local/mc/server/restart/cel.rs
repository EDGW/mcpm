//! Isolated Common Expression Language (CEL) compiler and evaluator for custom restart rules.

use crate::local::mc::crash_report::CrashReportType;
use cel::{Context, Env, ExecutionError, Program, RuntimeOptions, Value};
use chrono::{DateTime, FixedOffset};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use thiserror::Error;

use super::{
    engine::{ExecutionRecord, ExitRecord, within_window},
    model::{ExitKind, ExitMatcher, parse_duration},
};

const MAX_CEL_ITERATIONS: u64 = 10_000;

/// Pre-compiled CEL expression ready for repeated evaluation across server exits.
#[derive(Debug)]
pub(super) struct CompiledExpression {
    program: Program,
}

impl CompiledExpression {
    /// Evaluates the expression against the provided context and requires a boolean result.
    pub(super) fn evaluate(&self, context: &Context<'_, '_>) -> Result<bool, CelEvalError> {
        let value = self
            .program
            .execute(context)
            .map_err(|source| CelEvalError::Runtime {
                message: source.to_string(),
            })?;
        match value {
            Value::Bool(matched) => Ok(matched),
            other => Err(CelEvalError::NonBooleanResult {
                got: format!("{other:?}"),
            }),
        }
    }
}

/// Compiles a CEL expression in the standard restart environment.
pub(super) fn compile(source: &str) -> Result<CompiledExpression, CelCompileError> {
    let program = shared_env()
        .compile(source)
        .map_err(|source| CelCompileError::Parse {
            message: source.to_string(),
        })?;
    Ok(CompiledExpression { program })
}

/// Snapshot of exit, crash, run, and history state bound into one CEL evaluation context.
pub(super) struct CelContextInput<'a> {
    pub exit_kind: ExitKind,
    pub exit_code: i64,
    pub exit_signal: i64,
    pub stop_requested: bool,
    pub crash_detected: bool,
    pub crash_parsed: bool,
    pub crash_type: Option<CrashReportType>,
    pub crash_desc: &'a str,
    pub crash_path: &'a str,
    pub crash_error: &'a str,
    pub uptime: Duration,
    pub attempt: u32,
    pub total_restarts: u32,
    pub consecutive_crashes: u32,
    pub rcon_ever_connected: bool,
    pub now: DateTime<FixedOffset>,
    pub now_instant: Instant,
    pub executions: &'a [ExecutionRecord],
    pub exits: &'a [ExitRecord],
}

/// Builds a fresh CEL evaluation [`Context`] with non-null `exit`, `crash`, `run`, `now`
/// bindings plus `executed(rules, window)` and `exits(types, window)` history functions.
pub(super) fn build_context(input: &CelContextInput<'_>) -> Result<Context<'static, 'static>, CelEvalError> {
    let mut context = Context::with_env(shared_env());

    let exit_map: HashMap<&'static str, Value> = HashMap::from([
        ("type", Value::from(input.exit_kind.as_str())),
        ("code", Value::from(input.exit_code)),
        ("signal", Value::from(input.exit_signal)),
        ("stop_requested", Value::from(input.stop_requested)),
    ]);
    context.add_variable_from_value("exit", exit_map);

    let crash_type_str = match input.crash_type {
        Some(CrashReportType::Watchdog) => "watchdog",
        Some(CrashReportType::Others) => "others",
        Some(CrashReportType::Unknown) => "unknown",
        None => "none",
    };
    let crash_map: HashMap<&'static str, Value> = HashMap::from([
        ("detected", Value::from(input.crash_detected)),
        ("parsed", Value::from(input.crash_parsed)),
        ("type", Value::from(crash_type_str)),
        ("desc", Value::from(input.crash_desc)),
        ("path", Value::from(input.crash_path)),
        ("error", Value::from(input.crash_error)),
    ]);
    context.add_variable_from_value("crash", crash_map);

    let uptime_chrono =
        chrono::Duration::from_std(input.uptime).unwrap_or(chrono::Duration::MAX);
    let run_map: HashMap<&'static str, Value> = HashMap::from([
        ("uptime", Value::Duration(uptime_chrono)),
        (
            "uptime_secs",
            Value::from(i64::try_from(input.uptime.as_secs()).unwrap_or(i64::MAX)),
        ),
        ("attempt", Value::from(i64::from(input.attempt))),
        (
            "total_restarts",
            Value::from(i64::from(input.total_restarts)),
        ),
        (
            "consecutive_crashes",
            Value::from(i64::from(input.consecutive_crashes)),
        ),
        ("rcon_ever_connected", Value::from(input.rcon_ever_connected)),
    ]);
    context.add_variable_from_value("run", run_map);
    context.add_variable_from_value("now", Value::Timestamp(input.now));

    let snapshot = Arc::new(HistorySnapshot {
        now_instant: input.now_instant,
        executions: input.executions.to_vec(),
        exits: input.exits.to_vec(),
    });

    let exec_snapshot = Arc::clone(&snapshot);
    context
        .add_function(
            "executed",
            move |rules: Arc<Vec<Value>>, window: Arc<String>| -> Result<i64, ExecutionError> {
                let mut names = Vec::with_capacity(rules.len());
                for item in rules.iter() {
                    match item {
                        Value::String(name) => names.push(name.as_str()),
                        other => {
                            return Err(ExecutionError::FunctionError {
                                function: "executed".to_owned(),
                                message: format!(
                                    "rule names must be strings, got {other:?}"
                                ),
                            });
                        }
                    }
                }
                let cutoff = parse_optional_window("executed", window.as_str())?;
                let count = exec_snapshot
                    .executions
                    .iter()
                    .filter(|record| {
                        within_window(record.at, exec_snapshot.now_instant, cutoff)
                            && (names.is_empty()
                                || names.iter().any(|name| *name == record.rule_name))
                    })
                    .count();
                Ok(i64::try_from(count).unwrap_or(i64::MAX))
            },
        )
        .map_err(|source| CelEvalError::Runtime {
            message: source.to_string(),
        })?;

    context
        .add_function(
            "exits",
            move |types: Arc<Vec<Value>>, window: Arc<String>| -> Result<i64, ExecutionError> {
                let mut matchers = Vec::with_capacity(types.len());
                for item in types.iter() {
                    match item {
                        Value::String(name) => {
                            let Some(matcher) = ExitMatcher::parse(name.as_str()) else {
                                return Err(ExecutionError::FunctionError {
                                    function: "exits".to_owned(),
                                    message: format!("unknown exit matcher {name:?}"),
                                });
                            };
                            matchers.push(matcher);
                        }
                        other => {
                            return Err(ExecutionError::FunctionError {
                                function: "exits".to_owned(),
                                message: format!(
                                    "exit types must be strings, got {other:?}"
                                ),
                            });
                        }
                    }
                }
                let cutoff = parse_optional_window("exits", window.as_str())?;
                let count = snapshot
                    .exits
                    .iter()
                    .filter(|record| {
                        within_window(record.at, snapshot.now_instant, cutoff)
                            && (matchers.is_empty()
                                || matchers
                                    .iter()
                                    .any(|m| m.matches(record.kind, record.crash_type)))
                    })
                    .count();
                Ok(i64::try_from(count).unwrap_or(i64::MAX))
            },
        )
        .map_err(|source| CelEvalError::Runtime {
            message: source.to_string(),
        })?;

    Ok(context)
}

#[derive(Debug, Error)]
pub enum CelCompileError {
    #[error("failed to compile CEL condition: {message}")]
    Parse { message: String },
}

#[derive(Debug, Error)]
pub enum CelEvalError {
    #[error("CEL runtime error: {message}")]
    Runtime { message: String },
    #[error("CEL condition must evaluate to bool, got {got}")]
    NonBooleanResult { got: String },
}

struct HistorySnapshot {
    now_instant: Instant,
    executions: Vec<ExecutionRecord>,
    exits: Vec<ExitRecord>,
}

fn shared_env() -> Arc<Env> {
    static ENV: OnceLock<Arc<Env>> = OnceLock::new();
    Arc::clone(ENV.get_or_init(|| {
        let mut env = Env::stdlib();
        env.set_options(RuntimeOptions::default().with_max_iterations(MAX_CEL_ITERATIONS));
        Arc::new(env)
    }))
}

fn parse_optional_window(
    function: &str,
    window: &str,
) -> Result<Option<Duration>, ExecutionError> {
    let trimmed = window.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    parse_duration(trimmed)
        .map(Some)
        .map_err(|source| ExecutionError::FunctionError {
            function: function.to_owned(),
            message: source.to_string(),
        })
}
