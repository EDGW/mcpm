//! Remote log subscription concurrency and shutdown ordering.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};

use serde_json::{Value, json};
use uuid::Uuid;

fn event(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}

#[test]
fn log_subscription_keeps_command_slot_free_and_drains_before_closed() {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .args(["--log-level", "trace", "remote", "--max-concurrency", "1"])
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    assert_eq!(event(&mut output)["status"], "ready");
    let logs = Uuid::new_v4();
    let command = Uuid::new_v4();
    writeln!(input, "{}", json!({"logs":{"guid":logs,"level":"debug"}})).unwrap();
    input.flush().unwrap();
    assert_eq!(
        event(&mut output),
        json!({"guid":logs,"status":"ready","contents":null})
    );
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    writeln!(input, "{}", json!({"guid":command,"command":"mod.list","contents":{"paths":[fixture,"missing.jar"],"loaders":["fabric"]}})).unwrap();
    drop(input);
    let mut log_finished = false;
    let mut command_finished = false;
    loop {
        let reply = event(&mut output);
        if reply["guid"] == logs.to_string() {
            assert!(!log_finished);
            match reply["status"].as_str().unwrap() {
                "data" => {}
                "finished" => {
                    assert!(reply["contents"].is_null());
                    log_finished = true;
                }
                other => panic!("unexpected logging status: {other}"),
            }
        } else if reply["guid"] == command.to_string() {
            assert!(!command_finished);
            match reply["status"].as_str().unwrap() {
                "data" => {}
                "finished" => command_finished = true,
                other => panic!("log subscription must not consume the command slot: {other}"),
            }
        } else {
            assert_eq!(
                reply,
                json!({"guid":Uuid::nil(),"status":"closed","contents":null})
            );
            assert!(log_finished && command_finished);
            break;
        }
    }
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success(), "{result:?}");
}
