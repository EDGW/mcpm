//! Server source compatibility, archive selection, cancellation and local provider boundaries.

use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn server(root: &Path, loader: &str, version: &str) {
    fs::create_dir_all(root.join(".mcpm")).unwrap();
    fs::create_dir_all(root.join("mods")).unwrap();
    fs::write(
        root.join(".mcpm/server.json"),
        serde_json::to_vec(&json!({
            "version": 1, "minecraft_version": version,
            "loader": {"type": loader, "version": "1.0"}
        }))
        .unwrap(),
    )
    .unwrap();
}

fn jar(root: &Path, filename: &str) {
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        root.join("mods").join(filename),
    )
    .unwrap();
}

fn remote(root: &Path, command: &str, contents: Value, cancel: bool) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .arg("remote")
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let guid = uuid::Uuid::from_u128(1);
    let mut input = child.stdin.take().unwrap();
    let mut messages = format!(
        "{}\n",
        json!({"guid":guid,"command":command,"contents":contents})
    );
    if cancel {
        messages.push_str(&format!("{}\n", json!({"cancel":guid})));
    }
    input.write_all(messages.as_bytes()).unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn compatibility_defaults_come_from_remote_server_loader_and_minecraft_version() {
    let root = tempfile::tempdir().unwrap();
    for (loader, version) in [
        ("fabric", "1.21.1"),
        ("forge", "1.20.1"),
        ("neoforge", "1.21.3"),
    ] {
        let path = format!("server '{loader}' 中文");
        server(&root.path().join(&path), loader, version);
        for command in ["server.mods.source.search", "server.mods.source.files"] {
            let replies = remote(root.path(), command, json!({"server":path}), false);
            let data: Vec<_> = replies
                .iter()
                .filter(|reply| reply["status"] == "data")
                .collect();
            assert_eq!(data.len(), 1, "{replies:?}");
            assert_eq!(
                data[0]["contents"],
                json!({"version":version, "loader":loader})
            );
            assert!(replies.iter().any(|reply| reply["status"] == "finished"));
        }
    }
}

#[test]
fn identification_selects_outer_archives_once_and_keeps_valid_paths_after_input_failures() {
    let root = tempfile::tempdir().unwrap();
    let path = "remote server ' 中文";
    let selected = "quoted ' mod.jar.disabled";
    let server_root = root.path().join(path);
    server(&server_root, "fabric", "1.21.1");
    jar(&server_root, selected);
    fs::write(server_root.join("mods/broken.jar"), b"invalid archive").unwrap();
    let replies = remote(
        root.path(),
        "server.mods.source.identify",
        json!({
            "server":path, "parallelism":2, "selection":{"queries":[
                {"kind":"modid", "value":"omnibus_nested"},
                {"kind":"file", "value":format!("mods/{selected}")},
                {"kind":"file", "value":"../escape.jar"},
                {"kind":"modid", "value":"not_present"}
            ]}
        }),
        false,
    );
    let events: Vec<_> = replies
        .iter()
        .filter(|reply| reply["status"] == "data")
        .map(|reply| &reply["contents"])
        .collect();
    let progress: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "scan")
        .map(|event| {
            (
                event["completed"].as_u64().unwrap(),
                event["total"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(progress, [(0, 2), (1, 2), (2, 2)]);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "error")
            .count(),
        3,
        "{events:?}"
    );
    let done = events.last().unwrap();
    assert_eq!(done["event"], "done");
    assert_eq!(done["failed"], 3);
    assert_eq!(
        done["paths"],
        json!([Path::new(path).join("mods").join(selected)])
    );
    assert!(replies.iter().any(|reply| reply["status"] == "finished"));
}

#[test]
fn cancellation_terminates_selection_without_business_completion() {
    let root = tempfile::tempdir().unwrap();
    let server_root = root.path().join("server");
    server(&server_root, "fabric", "1.21.1");
    for index in 0..128 {
        jar(&server_root, &format!("{index}.jar"));
    }
    let replies = remote(
        root.path(),
        "server.mods.source.identify",
        json!({
            "server":"server", "parallelism":1,
            "selection":{"queries":[{"kind":"modid", "value":"omnibus_nested"}]}
        }),
        true,
    );
    assert!(
        replies
            .iter()
            .any(|reply| reply["status"] == "error" && reply["contents"]["code"] == "cancelled"),
        "{replies:?}"
    );
    assert!(
        !replies
            .iter()
            .any(|reply| reply["status"] == "data" && reply["contents"]["event"] == "done")
    );
    assert!(!replies.iter().any(|reply| reply["status"] == "finished"));
}

#[cfg(unix)]
#[test]
fn ssh_reads_remote_bytes_but_keeps_provider_options_and_failures_local() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let client = tempfile::tempdir().unwrap();
    let server_root = root.path().join("server ' 中文");
    server(&server_root, "fabric", "1.21.1");
    jar(&server_root, "selected.jar");
    let capture = root.path().join("requests.jsonl");
    let ssh = root.path().join("ssh");
    fs::write(
        &ssh,
        format!(
            "#!/bin/sh\ncd {} || exit 1\nfor last do :; done\ntee {} | /bin/sh -c \"$last\"\n",
            shlex::try_quote(root.path().to_str().unwrap()).unwrap(),
            shlex::try_quote(capture.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    let connection = shlex::try_join([ssh.to_str().unwrap(), "test@host"]).unwrap();
    let output: Output = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .current_dir(client.path())
        .env("HOME", root.path().join("home"))
        .env("USERPROFILE", root.path().join("home"))
        .args([
            "--raw",
            "--log-level",
            "off",
            "server",
            "mods",
            "source",
            "identify",
            "--server",
            "server ' 中文",
            "--file",
            "selected.jar",
            "--source",
            "invalid-provider@secret-sentinel",
            "--ssh",
            &connection,
            "--ssh-mcpm",
            env!("CARGO_BIN_EXE_mcpm-tools"),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("source must be modrinth"), "{error}");
    assert!(!error.contains("remote error:"), "{error}");
    let requests = fs::read_to_string(capture).unwrap();
    assert!(requests.contains("server.mods.source.identify"));
    assert!(requests.contains("mod.source.identify"));
    assert!(!requests.contains("invalid-provider"));
    assert!(!requests.contains("secret-sentinel"));
    let messages: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let summary = messages
        .iter()
        .find(|request| request["command"] == "mod.source.identify")
        .unwrap();
    assert_eq!(
        summary["contents"]["paths"],
        json!([Path::new("server ' 中文/mods/selected.jar")])
    );
    assert!(!client.path().join("server ' 中文").exists());
}
