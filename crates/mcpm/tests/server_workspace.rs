//! Legacy server-mod endpoints reject Remote targets without following their connection.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};

fn write(root: &Path, name: &str, value: Value) {
    fs::create_dir_all(root.join(".mcpm")).unwrap();
    fs::write(
        root.join(".mcpm").join(name),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}

fn success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn all_server_endpoints_reject_remote_targets_without_following_their_connection() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "mcpm.json",
        json!({"version":1,"type":"remote"}),
    );
    // The server.json would even parse as Local if a handler skipped the type gate.
    local_payload(root.path());
    let before = fs::read(root.path().join(".mcpm/server.json")).unwrap();
    for (command, contents) in [
        ("server.mods.list", json!({})),
        (
            "server.mods.cache",
            json!({"server":".","selection":{"all":true},"writing":{}}),
        ),
        (
            "server.mods.info",
            json!({"selection":{"queries":[{"kind":"file","value":"a.jar"}]}}),
        ),
        (
            "server.mods.deps",
            json!({"selection":{"queries":[{"kind":"file","value":"a.jar"}]}}),
        ),
        ("server.mods.source.search", json!({"server":"."})),
        ("server.mods.source.files", json!({"server":"."})),
        (
            "server.mods.source.identify",
            json!({"server":".","selection":{"all":true}}),
        ),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
            .arg("remote")
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "{}",
            json!({"guid":uuid::Uuid::from_u128(1),"command":command,"contents":contents})
        )
        .unwrap();
        let output = child.wait_with_output().unwrap();
        success(&output);
        let replies: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(
            replies.iter().any(|reply| reply["status"] == "error"
                && reply["contents"]
                    .to_string()
                    .contains("requires a Local Server")),
            "{command}: {replies:?}"
        );
        assert!(
            !replies.iter().any(|reply| reply["status"] == "finished"),
            "{command}: {replies:?}"
        );
        assert_eq!(
            fs::read(root.path().join(".mcpm/server.json")).unwrap(),
            before
        );
        assert!(!root.path().join("mods").exists());
    }
}

fn local_payload(root: &Path) {
    write(
        root,
        "server.json",
        json!({"version":2,"minecraft_version":"1.21.1","loader":{"type":"fabric","version":"0.16.10"}}),
    );
}
