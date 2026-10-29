//! Per-call log forwarding, interleaving and shutdown ordering.

mod common;

use std::{
    path::Path,
    process::{Command, Stdio},
};

use serde_json::json;
use uuid::Uuid;

#[test]
fn call_logs_are_forwarded_for_their_call_and_drain_before_closed() {
    let home = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .args(["--log-level", "trace", "remote"])
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut remote = common::Remote::from_child(child);
    assert_eq!(remote.next_frame()["status"], "ready");
    let command = Uuid::new_v4();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    remote.send(json!({
        "guid": command,
        "operation": "mod.list",
        "level": "debug",
        "input": {"paths": [fixture, "missing.jar"], "loaders": ["fabric"]},
    }));
    let finished = remote.finish();
    let mut logs = 0;
    let mut completed = false;
    for frame in &finished.frames {
        if frame["guid"] == command.to_string() {
            assert!(!completed, "no frame may follow the call's completion");
            match frame["status"].as_str().unwrap() {
                "log" => {
                    assert!(
                        matches!(
                            frame["contents"]["level"].as_str().unwrap(),
                            "error" | "warn" | "info" | "debug"
                        ),
                        "{}",
                        frame["contents"]["level"]
                    );
                    logs += 1;
                }
                "data" => {}
                "finished" => completed = true,
                other => panic!("unexpected frame status: {other}"),
            }
        }
    }
    assert!(
        completed,
        "the call must finish before the connection closes"
    );
    assert!(logs > 0, "the call's own records must be forwarded");
    assert_eq!(finished.frames.last().unwrap()["status"], "closed");
    assert!(finished.stderr.is_empty(), "{:?}", finished.frames);
}
