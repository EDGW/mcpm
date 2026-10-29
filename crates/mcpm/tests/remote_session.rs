//! End-to-end typed requests, interleaved results, command-local failures and capability discovery.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use uuid::Uuid;
const LIST: &str = "mods.jar.read";

fn event(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0);
    assert!(line.ends_with('\n'));
    assert!(!line.contains('\u{1b}'));
    serde_json::from_str(&line).unwrap()
}

/// Build identity both ends must agree on; read from the executable under test.
fn build_version() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .arg("--version")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    stdout
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned()
}

fn scope(paths: Value, loaders: Value) -> Value {
    json!({"kind":"paths", "directory":".", "paths":paths, "loaders":loaders})
}

#[test]
fn concurrent_typed_requests_and_failures_share_one_process() {
    let empty = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .arg("remote")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let ready = event(&mut output);
    assert_eq!(ready["guid"], Uuid::nil().to_string());
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["contents"]["version"], mcpm_exec::protocol::VERSION);
    assert_eq!(ready["contents"]["build"], build_version());
    let commands = ready["contents"]["operations"].as_array().unwrap();
    assert!(commands.iter().any(|id| id == LIST));
    assert!(
        commands
            .windows(2)
            .all(|ids| ids[0].as_str().unwrap() < ids[1].as_str().unwrap())
    );
    let cases = [
        (LIST, json!({"scope":scope(json!([fixture]), json!(["fabric"]))})),
        (LIST, json!({"scope":scope(json!([42]), json!([]))})),
        ("mod.unsupported", json!({"path":"missing.jar"})),
        (LIST, json!({"scope":scope(json!(["missing.jar"]), json!([]))})),
        (LIST, json!({"scope":scope(json!([empty.path()]), json!([]))})),
        (LIST, json!({"args":["mods","list"]})),
    ];
    // Submit every request before reading any command response.
    for (index, (command, args)) in cases.iter().enumerate() {
        writeln!(
            input,
            "{}",
            json!({"guid":Uuid::from_u128(index as u128+1),"operation":command,"level":"off","input":args})
        )
        .unwrap();
    }
    input.flush().unwrap();
    let mut received = HashMap::<usize, Vec<Value>>::new();
    let mut finished = 0;
    while finished < cases.len() {
        let value = event(&mut output);
        let id = value["guid"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap()
            .as_u128() as usize;
        assert!((1..=cases.len()).contains(&id));
        if value["status"] == "finished" || value["status"] == "failed" {
            finished += 1;
        }
        received.entry(id).or_default().push(value);
    }
    for index in 0..cases.len() {
        let replies = &received[&(index + 1)];
        if index == 2 {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0]["status"], "failed");
            assert_eq!(replies[0]["contents"]["failure"], "dispatch");
            assert_eq!(replies[0]["contents"]["error"]["kind"], "unknown_operation");
            continue;
        }
        if index == 1 || index == 5 {
            assert_eq!(replies.len(), 1, "{replies:?}");
            assert_eq!(replies[0]["status"], "failed");
            assert_eq!(replies[0]["contents"]["failure"], "dispatch");
            continue;
        }
        let terminal = replies.last().unwrap();
        assert_eq!(terminal["status"], "finished", "{replies:?}");
        let events: Vec<_> = replies
            .iter()
            .filter(|reply| reply["status"] == "data")
            .map(|reply| &reply["contents"])
            .collect();
        let items: Vec<_> = events
            .iter()
            .filter(|value| value["event"] == "item")
            .collect();
        match index {
            0 => {
                assert_eq!(items.len(), 1, "{events:?}");
                assert_eq!(items[0]["record"]["metadata"][0]["mods"][0]["modid"], "omnibus");
                assert_eq!(terminal["contents"]["files"], 1);
            }
            3 => {
                assert_eq!(items.len(), 1, "{events:?}");
                assert_eq!(items[0]["record"]["status"], "failed");
                assert_eq!(terminal["contents"]["input_errors"], 1);
            }
            4 => {
                assert!(items.is_empty(), "{events:?}");
                assert_eq!(terminal["contents"]["files"], 0);
            }
            _ => unreachable!(),
        }
    }
    assert!(child.try_wait().unwrap().is_none());
    // An earlier request error must not damage a later request.
    writeln!(
        input,
        "{}",
        json!({"guid":Uuid::from_u128(7),"operation":LIST,"level":"off","input":{"scope":scope(json!([empty.path()]), json!([]))}})
    )
    .unwrap();
    input.flush().unwrap();
    loop {
        let value = event(&mut output);
        if value["status"] == "finished" {
            assert_eq!(value["contents"]["files"], 0);
            break;
        }
    }
    drop(input);
    assert_eq!(
        event(&mut output),
        json!({"guid":Uuid::nil(),"status":"closed","contents":null})
    );
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(result.stderr.is_empty(), "{:?}", result);
}

#[test]
fn duplicate_ids_and_broken_frames_fail_the_session_instead_of_aliasing_requests() {
    let request =
        json!({"guid":Uuid::from_u128(1),"operation":LIST,"level":"off","input":{}}).to_string();
    for text in [format!("{request}\n{request}\n"), request] {
        let root = tempfile::tempdir().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
            .current_dir(root.path())
            .arg("remote")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let events = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(events.iter().any(|event| event["status"] == "failed"
            && event["guid"] == Uuid::nil().to_string()
            && event["contents"]["failure"] == "dispatch"
            && event["contents"]["error"]["kind"] == "protocol"));
        assert!(!events.iter().any(|event| event["status"] == "closed"));
    }
}
