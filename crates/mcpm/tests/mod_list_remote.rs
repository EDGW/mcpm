//! Structured `mods.jar.read` checks for exact records, failures and recursive metadata.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn run(root: &Path, args: Value) -> (Vec<Value>, Value, Vec<Value>) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
    command.current_dir(root).arg("remote");
    let mut remote = common::Remote::start(&mut command);
    remote.send(serde_json::json!({
        "guid":"00000000-0000-0000-0000-000000000001",
        "operation":"mods.jar.read",
        "level":"off",
        "input":args
    }));
    let finished = remote.finish();
    assert!(finished.stderr.is_empty(), "{:?}", finished.frames);
    let frames = finished.frames;
    let records: Vec<_> = frames
        .iter()
        .filter(|reply| {
            reply["status"] == "data"
                && reply["contents"]["event"] == "item"
        })
        .map(|reply| reply["contents"]["record"].clone())
        .collect();
    let summary = frames
        .iter()
        .find(|reply| reply["status"] == "finished")
        .expect("finished frame")
        .clone();
    (records, summary["contents"].clone(), frames)
}

fn record<'a>(records: &'a [Value], path: &str) -> &'a Value {
    records
        .iter()
        .find(|record| record["path"] == path)
        .unwrap_or_else(|| panic!("missing record {path}: {records:?}"))
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
    let (records, summary, frames) = run(
        root.path(),
        serde_json::json!({
            "scope":{
                "kind":"paths",
                "directory":".",
                "paths":[".","missing.jar","invalid.txt","a.jar"],
                "loaders":["forge"]
            },
            "parallelism":3,
            "cache":{"kind":"disabled"}
        }),
    );
    assert_eq!(records.len(), 6, "{frames:?}");
    assert_eq!(summary["files"], 6);
    assert_eq!(summary["input_errors"], 2);
    assert_eq!(summary["failed"], 1, "{frames:?}");

    let success = record(&records, "./a.jar");
    assert_eq!(success["status"], "success");
    assert_eq!(success["enabled"], true);
    assert_eq!(success["metadata"].as_array().unwrap().len(), 1);
    assert_eq!(success["metadata"][0]["loader"], "forge");
    assert_eq!(success["metadata"][0]["mods"][0]["modid"], "omnibus");
    assert_eq!(
        success["metadata"][0]["nested"][0]["meta"]["nested"][0]["meta"]["mods"][0]["modid"],
        "omnibus_leaf"
    );

    let failed = record(&records, "./b.jar");
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["errors"].as_array().unwrap().len(), 1);
    assert_eq!(failed["errors"][0]["loader"], "forge");
    assert!(
        failed["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("b.jar")
    );

    let unrecognized = record(&records, "./c.jar.disabled");
    assert_eq!(unrecognized["status"], "unrecognized");
    assert_eq!(unrecognized["enabled"], false);
    assert_eq!(unrecognized["metadata"], serde_json::json!([]));
    assert_eq!(unrecognized["errors"], serde_json::json!([]));

    for path in ["missing.jar", "invalid.txt"] {
        let record = record(&records, path);
        assert_eq!(record["status"], "failed");
        assert_eq!(record["errors"].as_array().unwrap().len(), 1);
        assert!(record["errors"][0]["loader"].is_null());
    }
    assert_eq!(record(&records, "a.jar")["metadata"], success["metadata"]);
}
