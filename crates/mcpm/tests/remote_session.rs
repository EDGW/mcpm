//! End-to-end typed requests, interleaved results, command-local failures and capability discovery.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use uuid::Uuid;
const LIST: &str = "mod.list";

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
        (
            LIST,
            json!({"paths":[fixture],"loaders":["fabric"]}),
            vec!["count", "item", "done"],
            true,
        ),
        (LIST, json!({"paths":42}), vec!["error", "done"], false),
        (
            "mod.unsupported",
            json!({"path":"missing.jar"}),
            vec!["unsupported_command"],
            false,
        ),
        (
            LIST,
            json!({"paths":["missing.jar"]}),
            vec!["count", "item", "done"],
            false,
        ),
        (
            LIST,
            json!({"paths":[empty.path()]}),
            vec!["count", "done"],
            true,
        ),
        (
            LIST,
            json!({"args":["mods","list"]}),
            vec!["error", "done"],
            false,
        ),
    ];
    // Submit every request before reading any command response.
    for (index, (command, args, _, _)) in cases.iter().enumerate() {
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
    for (index, (_, _, expected, success)) in cases.iter().enumerate() {
        let replies = &received[&(index + 1)];
        assert!(
            replies
                .iter()
                .all(|reply| reply.as_object().unwrap().len() == 3)
        );
        if index == 2 {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0]["status"], "failed");
            assert_eq!(replies[0]["contents"]["failure"], "dispatch");
            assert_eq!(replies[0]["contents"]["error"]["kind"], "unknown_operation");
            continue;
        }
        assert_eq!(replies.last().unwrap()["status"], "finished");
        let events: Vec<_> = replies
            .iter()
            .filter(|reply| reply["status"] == "data")
            .map(|reply| &reply["contents"])
            .collect();
        assert_eq!(
            events
                .iter()
                .map(|value| value["event"].as_str().unwrap())
                .collect::<Vec<_>>(),
            *expected
        );
        assert_eq!(events.last().unwrap()["success"], *success);
        if index == 0 {
            assert_eq!(
                events[1]["record"]["metadata"][0]["mods"][0]["modid"],
                "omnibus"
            );
        }
        if index == 1 || index == 5 {
            assert!(
                events[0]["message"]
                    .as_str()
                    .unwrap()
                    .contains("invalid list arguments")
            );
        }
        if index == 3 {
            assert_eq!(events[1]["record"]["status"], "failed");
        }
    }
    assert!(child.try_wait().unwrap().is_none());
    // An earlier request error must not damage a later request.
    writeln!(
        input,
        "{}",
        json!({"guid":Uuid::from_u128(7),"operation":LIST,"level":"off","input":{"paths":[empty.path()]}})
    )
    .unwrap();
    input.flush().unwrap();
    assert_eq!(
        event(&mut output),
        json!({"guid":Uuid::from_u128(7),"status":"data","contents":{"event":"count","count":0}})
    );
    assert_eq!(
        event(&mut output),
        json!({"guid":Uuid::from_u128(7),"status":"data","contents":{"event":"done","success":true,"cache":{"fully_cached":0,"partially_cached":0,"total":0}}})
    );
    assert_eq!(event(&mut output)["status"], "finished");
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
