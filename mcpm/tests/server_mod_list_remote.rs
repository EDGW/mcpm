//! Server list transport preserves selected archive identities, unique scan counts and local presentation.

use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn fixture(root: &Path) {
    fs::create_dir_all(root.join(".mcpm")).unwrap();
    fs::create_dir(root.join("mods")).unwrap();
    fs::write(
        root.join(".mcpm/server.json"),
        serde_json::to_vec(&json!({
            "version":1,"minecraft_version":"1.21.1","loader":{"type":"fabric","version":"0.16.10"}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        root.join("mods/quoted ' mod.jar.disabled"),
    )
    .unwrap();
    fs::write(root.join("mods/broken.jar"), b"bad archive").unwrap();
}

fn remote(root: &Path, selection: Value) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("remote")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{}", json!({"guid":uuid::Uuid::from_u128(1),"command":"server.mods.list","contents":{"selection":selection,"parallelism":2}})).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        replies[replies.len() - 2]["status"],
        "finished",
        "{replies:?}"
    );
    replies
        .into_iter()
        .filter(|reply| reply["status"] == "data")
        .map(|reply| reply["contents"].clone())
        .collect()
}

#[test]
fn selected_records_and_failure_counts_are_independent_of_output_multiplicity() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let events = remote(
        root.path(),
        json!({"queries":[
            {"kind":"modid","value":"omnibus_nested"},
            {"kind":"modid","value":"omnibus_nested"},
            {"kind":"file","value":"mods/quoted ' mod.jar.disabled"},
            {"kind":"file","value":"../escape.jar"}
        ]}),
    );
    assert_eq!(events[0], json!({"event":"scope","loaders":["fabric"]}));
    let scan: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "scan")
        .map(|e| {
            (
                e["completed"].as_u64().unwrap(),
                e["total"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(scan, [(0, 2), (1, 2), (2, 2)]);
    let items: Vec<_> = events.iter().filter(|e| e["event"] == "item").collect();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["record"]["records"], items[1]["record"]["records"]);
    assert!(!items[0]["record"]["records"].as_array().unwrap().is_empty());
    assert!(items[2]["record"].get("records").is_none());
    assert_eq!(items[2]["record"]["enabled"], false);
    // The failed index-only file is not an item; done remains the scan's unique accounting.
    assert_eq!(
        events.last().unwrap(),
        &json!({"event":"done","files":3,"input_errors":1,"failed":1,"cache":{"fully_cached":0,"partially_cached":0,"total":2}})
    );
    assert_eq!(events.iter().filter(|e| e["event"] == "error").count(), 2);
    let empty = tempfile::tempdir().unwrap();
    fixture(empty.path());
    fs::remove_file(empty.path().join("mods/broken.jar")).unwrap();
    fs::remove_file(empty.path().join("mods/quoted ' mod.jar.disabled")).unwrap();
    let events = remote(empty.path(), json!({}));
    assert_eq!(events[1], json!({"event":"scan","completed":0,"total":0}));
    assert_eq!(
        events.last().unwrap(),
        &json!({"event":"done","files":0,"input_errors":0,"failed":0,"cache":{"fully_cached":0,"partially_cached":0,"total":0}})
    );
}

#[cfg(unix)]
#[test]
fn ssh_keeps_nested_selection_filtering_and_paths_equivalent_to_local_lists() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::Builder::new()
        .prefix("mcpm server list ' ")
        .tempdir()
        .unwrap();
    fixture(root.path());
    let ssh = root.path().join("ssh");
    fs::write(
        &ssh,
        format!(
            "#!/bin/sh\ncd {} || exit 1\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
            shlex::try_quote(root.path().to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    let connection = shlex::try_join([ssh.to_str().unwrap(), "test@host"]).unwrap();
    for options in [
        vec![
            "--modid",
            "omnibus_nested",
            "--modid",
            "omnibus_nested",
            "--expanded",
        ],
        vec!["--all", "--filter", "@modid omnibus_leaf", "--expanded"],
        vec![
            "--file",
            "mods/quoted ' mod.jar.disabled",
            "--path-style",
            "full",
            "--expanded",
        ],
        vec![
            "--file",
            "../escape.jar",
            "--file",
            "quoted ' mod.jar.disabled",
        ],
        vec![],
    ] {
        let run = |remote: bool| {
            let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm"));
            process
                .args([
                    "--raw",
                    "--remote-logs",
                    "--log-level",
                    "off",
                    "server",
                    "mods",
                    "list",
                    "--no-cache",
                    "--parallelism",
                    "2",
                ])
                .args(&options);
            if remote {
                process.args([
                    "--ssh",
                    &connection,
                    "--ssh-mcpm",
                    env!("CARGO_BIN_EXE_mcpm"),
                ]);
            } else {
                process.current_dir(root.path());
            }
            process.stdin(Stdio::null()).output().unwrap()
        };
        let local = run(false);
        let remote = run(true);
        assert_eq!(local.status.code(), remote.status.code(), "{remote:?}");
        assert_eq!(local.stdout, remote.stdout, "{options:?}");
        assert_eq!(
            String::from_utf8_lossy(&local.stderr),
            String::from_utf8_lossy(&remote.stderr).replace("remote error:", "error:"),
            "{options:?}"
        );
    }
}
