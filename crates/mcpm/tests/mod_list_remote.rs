//! Structured mod.list protocol checks for exact counts, failures and recursive metadata.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

fn run(root: &Path, args: Value, count: usize, success: bool) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .current_dir(root)
        .arg("remote")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        serde_json::json!({"guid":"00000000-0000-0000-0000-000000000001","command":"mod.list","contents":args})
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output);
    assert!(!output.stdout.contains(&0x1b));
    assert!(output.stdout.ends_with(b"\n"));
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), count + 5, "{events:?}");
    assert_eq!(events[0]["status"], "ready");
    let envelopes = events;
    assert!(
        envelopes
            .iter()
            .all(|reply| reply.as_object().unwrap().len() == 3)
    );
    let events: Vec<_> = envelopes
        .iter()
        .filter(|reply| reply["status"] == "data")
        .map(|reply| reply["contents"].clone())
        .collect();
    assert_eq!(
        events[0],
        serde_json::json!({"event":"count","count":count})
    );
    assert_eq!(events[count + 1]["event"], "done");
    assert_eq!(events[count + 1]["success"], success);
    assert_eq!(envelopes[count + 3]["status"], "finished");
    assert_eq!(envelopes[count + 4]["status"], "closed");
    let mut records = vec![None; count];
    for event in &events[1..count + 1] {
        assert_eq!(event["event"], "item");
        let index = event["index"].as_u64().unwrap() as usize;
        assert!(index < count && records[index].is_none(), "{event}");
        records[index] = Some(event["record"].clone());
    }
    records.into_iter().map(Option::unwrap).collect()
}

#[test]
fn every_success_failure_and_unrecognized_file_has_exactly_one_record() {
    let root = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::copy(fixtures.join("complex-mod.jar"), root.path().join("a.jar")).unwrap();
    fs::write(root.path().join("b.jar"), b"invalid archive\n").unwrap();
    fs::copy(
        fixtures.join("dependency-problems/mods/fabric_only.jar"),
        root.path().join("c.jar.disabled"),
    )
    .unwrap();
    fs::write(root.path().join("invalid.txt"), b"not a mod").unwrap();
    let records = run(
        root.path(),
        serde_json::json!({"paths":[".","missing.jar","invalid.txt","a.jar"],"loaders":["forge"],"parallelism":3}),
        6,
        false,
    );
    assert_eq!(records[0]["status"], "success");
    assert_eq!(records[0]["path"], "./a.jar");
    assert_eq!(records[0]["enabled"], true);
    assert_eq!(records[0]["metadata"].as_array().unwrap().len(), 1);
    assert_eq!(records[0]["metadata"][0]["loader"], "forge");
    assert_eq!(records[0]["metadata"][0]["mods"][0]["modid"], "omnibus");
    assert_eq!(
        records[0]["metadata"][0]["nested"][0]["meta"]["nested"][0]["meta"]["mods"][0]["modid"],
        "omnibus_leaf"
    );
    assert_eq!(records[1]["path"], "./b.jar");
    assert_eq!(records[1]["status"], "failed");
    assert_eq!(records[1]["errors"].as_array().unwrap().len(), 1);
    assert_eq!(records[1]["errors"][0]["loader"], "forge");
    assert!(
        records[1]["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("b.jar")
    );
    assert_eq!(records[2]["status"], "unrecognized");
    assert_eq!(records[2]["enabled"], false);
    assert_eq!(records[2]["metadata"], serde_json::json!([]));
    assert_eq!(records[2]["errors"], serde_json::json!([]));
    for (record, path) in records[3..5].iter().zip(["missing.jar", "invalid.txt"]) {
        assert_eq!(record["path"], path);
        assert_eq!(record["status"], "failed");
        assert_eq!(record["errors"].as_array().unwrap().len(), 1);
        assert!(record["errors"][0]["loader"].is_null());
    }
    assert_eq!(records[5]["path"], "a.jar");
    assert_eq!(records[5]["metadata"], records[0]["metadata"]);
}
