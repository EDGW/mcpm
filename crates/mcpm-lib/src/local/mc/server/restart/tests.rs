//! Focused tests for restart policy decoding, validation, backoff progression, and CEL evaluation.

use super::*;
use crate::local::mc::{
    crash_report::{CrashReportAnalysis, CrashReportType},
    server::launch::LaunchExit,
};
use chrono::DateTime;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct PolicyDoc {
    #[serde(default)]
    restart_on: RestartPolicy,
}

fn parse_policy(toml_str: &str) -> RestartPolicy {
    toml::from_str::<PolicyDoc>(toml_str)
        .expect("valid restart_on TOML")
        .restart_on
}

fn fixed_now() -> DateTime<chrono::FixedOffset> {
    DateTime::parse_from_rfc3339("2026-11-27T12:00:00+08:00").unwrap()
}

fn observation(
    exit: LaunchExit,
    at: Instant,
    uptime: Duration,
    crash_report: Option<ObservedCrashReport>,
) -> ExitObservation {
    ExitObservation {
        exit,
        killed_by_mcpm: false,
        stop_requested: false,
        crash_report,
        uptime,
        rcon_ever_connected: true,
        now: fixed_now(),
        at,
    }
}

fn watchdog_report() -> ObservedCrashReport {
    ObservedCrashReport {
        path: "crash-reports/crash-watchdog.txt".to_owned(),
        analysis: Ok(CrashReportAnalysis {
            report_type: CrashReportType::Watchdog,
            desc: Some("Watching Server".to_owned()),
            time: None,
        }),
    }
}

#[test]
fn parses_and_validates_duration_and_rule_modes() {
    assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
    assert_eq!(
        parse_duration("1h30m15s").unwrap(),
        Duration::from_secs(5415)
    );
    assert!(parse_duration("").is_err());
    assert!(parse_duration("10w").is_err());

    // Default type = "simple" when omitted, plus exit_type list & single value.
    let policy = parse_policy(
        r#"
[[restart_on]]
name = "rule0"
exit_type = "crash"
max_runs = 3
window = "10m"

[[restart_on]]
name = "rule1"
type = "custom"
condition = "exit.type == 'crash' && crash.type == 'watchdog' && executed(['rule1'], '10m') < 3"
"#,
    );
    policy.validate().unwrap();
    assert_eq!(policy.rules.len(), 2);
    assert!(matches!(policy.rules[0].mode, RestartMode::Simple(_)));
    assert!(matches!(policy.rules[1].mode, RestartMode::Custom(_)));

    // Reject mode-mismatched fields.
    let bad = toml::from_str::<PolicyDoc>(
        r#"
[[restart_on]]
type = "simple"
condition = "true"
"#,
    );
    assert!(bad.is_err());

    // Reject duplicate rule names.
    let dup = parse_policy(
        r#"
[[restart_on]]
name = "same"

[[restart_on]]
name = "same"
"#,
    );
    assert!(dup.validate().is_err());

    // Reject invalid CEL syntax at validation time.
    let bad_cel = parse_policy(
        r#"
[[restart_on]]
type = "custom"
condition = "exit.type ==="
"#,
    );
    assert!(bad_cel.validate().is_err());
}

#[test]
fn simple_rule_respects_sliding_window_and_exit_codes() {
    let policy = parse_policy(
        r#"
[[restart_on]]
name = "oom"
exit_type = "crash"
exit_codes = [1, 137]
max_runs = 2
window = "10m"
delay = "3s"
"#,
    );
    let compiled = CompiledRestartPolicy::compile(&policy).unwrap();
    let mut history = RestartHistory::new();
    let t0 = Instant::now();

    // Non-matching exit code does not restart.
    let out = compiled.evaluate(
        &observation(LaunchExit::Code(2), t0, Duration::from_secs(30), None),
        &mut history,
    );
    assert_eq!(out.decision, RestartDecision::Stop { matched_rule: None });

    // First two matching crashes restart within window.
    let out1 = compiled.evaluate(
        &observation(LaunchExit::Code(1), t0, Duration::from_secs(30), None),
        &mut history,
    );
    assert_eq!(
        out1.decision,
        RestartDecision::Restart {
            rule: "oom".to_owned(),
            delay: Duration::from_secs(3),
            attempt: 2,
        }
    );

    let out2 = compiled.evaluate(
        &observation(
            LaunchExit::Code(137),
            t0 + Duration::from_secs(60),
            Duration::from_secs(30),
            None,
        ),
        &mut history,
    );
    assert_eq!(
        out2.decision,
        RestartDecision::Restart {
            rule: "oom".to_owned(),
            delay: Duration::from_secs(3),
            attempt: 3,
        }
    );

    // Third crash inside 10m window exhausts budget -> stops.
    let out3 = compiled.evaluate(
        &observation(
            LaunchExit::Code(1),
            t0 + Duration::from_secs(120),
            Duration::from_secs(30),
            None,
        ),
        &mut history,
    );
    assert_eq!(out3.decision, RestartDecision::Stop { matched_rule: None });

    // Crash after 10m+1s slides past the first execution -> restarts again.
    let out4 = compiled.evaluate(
        &observation(
            LaunchExit::Code(1),
            t0 + Duration::from_secs(601),
            Duration::from_secs(30),
            None,
        ),
        &mut history,
    );
    assert_eq!(
        out4.decision,
        RestartDecision::Restart {
            rule: "oom".to_owned(),
            delay: Duration::from_secs(3),
            attempt: 4,
        }
    );
}

#[test]
fn backoff_scales_caps_resets_and_stops_at_max_consecutive() {
    let policy = parse_policy(
        r#"
[[restart_on]]
name = "loop-guard"
type = "backoff"
exit_type = "crash"
initial_delay = "5s"
max_delay = "15s"
multiplier = 2.0
reset_after = "5m"
max_consecutive = 3
"#,
    );
    let compiled = CompiledRestartPolicy::compile(&policy).unwrap();
    let mut history = RestartHistory::new();
    let t0 = Instant::now();

    let d1 = compiled.evaluate(
        &observation(LaunchExit::Code(1), t0, Duration::from_secs(20), None),
        &mut history,
    );
    assert_eq!(
        d1.decision,
        RestartDecision::Restart {
            rule: "loop-guard".to_owned(),
            delay: Duration::from_secs(5),
            attempt: 2,
        }
    );

    let d2 = compiled.evaluate(
        &observation(
            LaunchExit::Code(1),
            t0 + Duration::from_secs(30),
            Duration::from_secs(20),
            None,
        ),
        &mut history,
    );
    assert_eq!(
        d2.decision,
        RestartDecision::Restart {
            rule: "loop-guard".to_owned(),
            delay: Duration::from_secs(10),
            attempt: 3,
        }
    );

    // Third consecutive crash hits max_delay cap (20s -> 15s).
    let d3 = compiled.evaluate(
        &observation(
            LaunchExit::Code(1),
            t0 + Duration::from_secs(60),
            Duration::from_secs(20),
            None,
        ),
        &mut history,
    );
    assert_eq!(
        d3.decision,
        RestartDecision::Restart {
            rule: "loop-guard".to_owned(),
            delay: Duration::from_secs(15),
            attempt: 4,
        }
    );

    // Fourth consecutive short crash exceeds max_consecutive (3) -> stops.
    let d4 = compiled.evaluate(
        &observation(
            LaunchExit::Code(1),
            t0 + Duration::from_secs(90),
            Duration::from_secs(20),
            None,
        ),
        &mut history,
    );
    assert_eq!(d4.decision, RestartDecision::Stop { matched_rule: None });

    // A crash after uptime >= reset_after (5m) resets the streak back to 5s.
    let d5 = compiled.evaluate(
        &observation(
            LaunchExit::Code(1),
            t0 + Duration::from_secs(400),
            Duration::from_secs(300),
            None,
        ),
        &mut history,
    );
    assert_eq!(
        d5.decision,
        RestartDecision::Restart {
            rule: "loop-guard".to_owned(),
            delay: Duration::from_secs(5),
            attempt: 5,
        }
    );
}

#[test]
fn custom_cel_supports_watchdog_history_stop_action_and_safe_defaults() {
    let policy = parse_policy(
        r#"
[[restart_on]]
name = "halt-fast-loop"
type = "custom"
action = "stop"
condition = "!run.rcon_ever_connected && exits(['crash'], '10m') >= 2"

[[restart_on]]
name = "watchdog-only"
type = "custom"
condition = "exit.type == 'crash' && crash.type == 'watchdog' && executed(['watchdog-only'], '10m') < 2"
delay = "2s"

[[restart_on]]
name = "broken-expr"
type = "custom"
condition = "unknown_var == 1"
"#,
    );
    let compiled = CompiledRestartPolicy::compile(&policy).unwrap();
    let mut history = RestartHistory::new();
    let t0 = Instant::now();

    // Crash without crash report: crash.type == 'none', never crashes on null;
    // falls through to broken-expr which produces a diagnostic and stops safely.
    let no_report = compiled.evaluate(
        &observation(LaunchExit::Code(1), t0, Duration::from_secs(40), None),
        &mut history,
    );
    assert_eq!(
        no_report.decision,
        RestartDecision::Stop { matched_rule: None }
    );
    assert_eq!(no_report.diagnostics.len(), 1);

    // Watchdog crashes match watchdog-only twice, then stop on the third.
    for expected_attempt in [2u32, 3u32] {
        let out = compiled.evaluate(
            &observation(
                LaunchExit::Code(1),
                t0 + Duration::from_secs(10),
                Duration::from_secs(120),
                Some(watchdog_report()),
            ),
            &mut history,
        );
        assert_eq!(
            out.decision,
            RestartDecision::Restart {
                rule: "watchdog-only".to_owned(),
                delay: Duration::from_secs(2),
                attempt: expected_attempt,
            }
        );
    }

    // Third watchdog crash inside 10m no longer matches watchdog-only.
    let third = compiled.evaluate(
        &observation(
            LaunchExit::Code(1),
            t0 + Duration::from_secs(20),
            Duration::from_secs(120),
            Some(watchdog_report()),
        ),
        &mut history,
    );
    assert_eq!(third.decision, RestartDecision::Stop { matched_rule: None });

    // Explicit action = "stop" rule short-circuits before later rules.
    let mut never_connected = observation(
        LaunchExit::Code(1),
        t0 + Duration::from_secs(30),
        Duration::from_secs(5),
        Some(watchdog_report()),
    );
    never_connected.rcon_ever_connected = false;
    let halted = compiled.evaluate(&never_connected, &mut history);
    assert_eq!(
        halted.decision,
        RestartDecision::Stop {
            matched_rule: Some("halt-fast-loop".to_owned()),
        }
    );
}
