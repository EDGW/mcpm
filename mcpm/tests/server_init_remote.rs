//! Remote initialization semantics: independent recognition, complete snapshots and atomic writes.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};

fn metadata(version: &str) -> Value {
    json!({"version":2,"minecraft_version":version,"loader":null})
}

fn fixture(name: &str, target: &Path) {
    fn copy(source: &Path, target: &Path) {
        fs::create_dir_all(target).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == ".mcpm" {
                continue;
            }
            let destination = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &destination);
            } else {
                fs::copy(entry.path(), destination).unwrap();
            }
        }
    }
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/server-init")
            .join(name),
        target,
    );
}

fn remote(root: &Path, requests: &[(&str, Value)]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .arg("remote")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for (index, (command, contents)) in requests.iter().enumerate() {
        writeln!(
            input,
            "{}",
            json!({"guid":uuid::Uuid::from_u128(index as u128 + 1),"command":command,"contents":contents})
        )
        .unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.last().unwrap()["status"], "closed");
    replies
}

fn data(replies: &[Value]) -> Vec<&Value> {
    replies
        .iter()
        .filter(|reply| reply["status"] == "data")
        .map(|reply| &reply["contents"])
        .collect()
}

fn succeeded(replies: &[Value]) {
    assert!(
        replies.iter().any(|reply| reply["status"] == "finished"),
        "{replies:?}"
    );
    assert!(
        !replies.iter().any(|reply| reply["status"] == "error"),
        "{replies:?}"
    );
}

fn failed(replies: &[Value]) {
    assert!(
        replies.iter().any(|reply| {
            reply["status"] == "error" && reply["contents"]["code"] == "handler_failed"
        }),
        "{replies:?}"
    );
    assert!(
        !replies.iter().any(|reply| reply["status"] == "finished"),
        "{replies:?}"
    );
    assert!(
        !data(replies)
            .iter()
            .any(|event| event["event"] == "initialized"),
        "{replies:?}"
    );
}

#[test]
fn complete_metadata_initializes_without_script_and_requires_explicit_vanilla() {
    let root = tempfile::tempdir().unwrap();
    // A dynamic, invalid script must never be consulted for complete input.
    fs::write(root.path().join("run.sh"), "if impossible; then broken\n").unwrap();
    let expected = metadata("1.21.1");
    let replies = remote(
        root.path(),
        &[("server.init", json!({"metadata":expected}))],
    );
    succeeded(&replies);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(root.path().join(".mcpm/server.json")).unwrap())
            .unwrap(),
        expected
    );
    let events = data(&replies);
    assert!(!events.iter().any(|event| event["phase"] == "recognizing"));

    let empty = tempfile::tempdir().unwrap();
    for invalid in [
        json!({"version":1,"minecraft_version":"1.21.1"}),
        json!({"version":1,"minecraft_version":"1.21.1","loader":{"type":"unsupported","version":"1"}}),
    ] {
        let replies = remote(
            empty.path(),
            &[("server.init", json!({"metadata":invalid}))],
        );
        failed(&replies);
        assert!(!empty.path().join(".mcpm").exists());
    }
}

#[test]
fn initialization_preserves_foreign_windows_start_snapshot_without_opening_its_paths() {
    let root = tempfile::tempdir().unwrap();
    let mut expected = metadata("1.21.1");
    expected["start"] = json!({
        "script":"run.bat",
        "executable":"Z:\\Program Files\\Java\\bin\\java.exe",
        "arguments":["", "-Dmessage=中文 \\\"quoted\\\" value", "Z:\\server files\\", "-jar", "server.jar", "$(touch escaped)"],
        "windows":true
    });
    let replies = remote(
        root.path(),
        &[("server.init", json!({"metadata":expected}))],
    );
    succeeded(&replies);
    let written: Value =
        serde_json::from_slice(&fs::read(root.path().join(".mcpm/server.json")).unwrap()).unwrap();
    assert_eq!(written, expected);
    let events = data(&replies);
    assert!(!events.iter().any(|event| event["phase"] == "recognizing"));
    let initialized = &events.last().unwrap()["server"];
    let replies = remote(root.path(), &[("server.info", json!({}))]);
    succeeded(&replies);
    let events = data(&replies);
    let info = events.last().unwrap();
    assert_eq!(info, &initialized);
    assert_eq!(info["start"]["directory"], ".");
    for field in ["executable", "arguments", "windows"] {
        assert_eq!(info["start"][field], expected["start"][field]);
    }
    assert!(!root.path().join("escaped").exists());
}

#[test]
fn recognition_of_unregistered_server_is_read_only() {
    let root = tempfile::tempdir().unwrap();
    fixture("forge", root.path());
    assert!(!root.path().join("mods").exists());
    assert!(!root.path().join(".mcpm").exists());
    let replies = remote(
        root.path(),
        &[("server.init.recognize", json!({"start_script":"run.sh"}))],
    );
    succeeded(&replies);
    let events = data(&replies);
    assert_eq!(
        events.last().unwrap()["candidates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!root.path().join("mods").exists());
    assert!(!root.path().join(".mcpm").exists());
}

#[test]
fn recognition_ignores_existing_configuration_and_selected_snapshot_is_independent() {
    let root = tempfile::tempdir().unwrap();
    fixture("forge", root.path());
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    let old = b"existing invalid configuration\n";
    fs::write(root.path().join(".mcpm/server.json"), old).unwrap();
    let replies = remote(
        root.path(),
        &[("server.init.recognize", json!({"start_script":"run.sh"}))],
    );
    succeeded(&replies);
    assert_eq!(
        fs::read(root.path().join(".mcpm/server.json")).unwrap(),
        old
    );
    let events = data(&replies);
    assert!(!events.iter().any(|event| event["phase"] == "writing"));
    let candidates = events.last().unwrap()["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    let mut selected = candidates[0]["metadata"].clone();
    assert_eq!(
        selected["version"], 1,
        "protocol v4 retains the legacy metadata envelope"
    );
    assert_eq!(selected["loader"]["type"], "forge");
    assert_eq!(selected["start"]["script"], "run.sh");
    assert!(selected["start"].get("directory").is_none());
    // Recognition produces a snapshot. Commit neither opens scripts nor requires a token.
    fs::remove_file(root.path().join("run.sh")).unwrap();
    fs::remove_dir_all(root.path().join("libraries")).unwrap();
    let replies = remote(
        root.path(),
        &[("server.init", json!({"metadata":selected,"force":true}))],
    );
    succeeded(&replies);
    // Wire v4 retains version 1, while persistence normalizes to disk version 2.
    selected["version"] = 2.into();
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(root.path().join(".mcpm/server.json")).unwrap())
            .unwrap(),
        selected
    );
}

#[test]
fn failed_validation_and_no_clobber_preserve_existing_configuration() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    let path = root.path().join(".mcpm/server.json");
    let old = b"old invalid JSON must survive failed forced initialization\n";
    fs::write(&path, old).unwrap();
    let mut invalid = metadata("1.21.1");
    invalid["start"] = json!({
        "script":"run.sh","executable":"java",
        "arguments":["embedded\u{0000}nul"],"windows":false
    });
    let replies = remote(
        root.path(),
        &[("server.init", json!({"metadata":invalid,"force":true}))],
    );
    failed(&replies);
    assert_eq!(fs::read(&path).unwrap(), old);
    let replies = remote(
        root.path(),
        &[("server.init", json!({"metadata":metadata("1.21.1")}))],
    );
    failed(&replies);
    assert_eq!(fs::read(&path).unwrap(), old);

    // A directory cannot be replaced even with --force; its contents survive.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("keep"), old).unwrap();
    let replies = remote(
        root.path(),
        &[(
            "server.init",
            json!({"metadata":metadata("1.21.1"),"force":true}),
        )],
    );
    failed(&replies);
    assert_eq!(fs::read(path.join("keep")).unwrap(), old);
}

#[test]
fn concurrent_initializers_have_one_winner_without_partial_or_overwritten_metadata() {
    let root = tempfile::tempdir().unwrap();
    let requests: Vec<_> = (0..8)
        .map(|index| {
            (
                "server.init",
                json!({"metadata":metadata(&format!("1.21.{index}"))}),
            )
        })
        .collect();
    let replies = remote(root.path(), &requests);
    let initialized: Vec<_> = data(&replies)
        .into_iter()
        .filter(|event| event["event"] == "initialized")
        .collect();
    assert_eq!(initialized.len(), 1, "{replies:?}");
    assert_eq!(
        replies
            .iter()
            .filter(|reply| reply["status"] == "finished")
            .count(),
        1,
        "{replies:?}"
    );
    assert_eq!(
        replies
            .iter()
            .filter(|reply| reply["status"] == "error")
            .count(),
        requests.len() - 1,
        "{replies:?}"
    );
    let written: Value =
        serde_json::from_slice(&fs::read(root.path().join(".mcpm/server.json")).unwrap()).unwrap();
    assert_eq!(
        written["minecraft_version"],
        initialized[0]["server"]["minecraft_version"]
    );
}

#[cfg(unix)]
struct SshFixture {
    source: tempfile::TempDir,
    client: tempfile::TempDir,
    connection: String,
    trace: PathBuf,
}

#[cfg(unix)]
impl SshFixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;

        let source = tempfile::Builder::new()
            .prefix("mcpm init ' 中文 ")
            .tempdir()
            .unwrap();
        let client = tempfile::tempdir().unwrap();
        let ssh = source.path().join("ssh");
        let trace = source.path().join("requests.jsonl");
        fs::write(
            &ssh,
            format!(
                "#!/bin/sh\ncd {} || exit 1\nfor last do :; done\ntee {} | /bin/sh -c \"$last\"\n",
                shlex::try_quote(source.path().to_str().unwrap()).unwrap(),
                shlex::try_quote(trace.to_str().unwrap()).unwrap(),
            ),
        )
        .unwrap();
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
        let connection = shlex::try_join([ssh.to_str().unwrap(), "test@host"]).unwrap();
        Self {
            source,
            client,
            connection,
            trace,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
            .current_dir(self.client.path())
            .args([
                "--raw",
                "--remote-logs",
                "--log-level",
                "off",
                "--ssh",
                &self.connection,
                "--ssh-mcpm",
                env!("CARGO_BIN_EXE_mcpm-tools"),
                "server",
                "init",
            ])
            .args(args)
            .output()
            .unwrap()
    }

    fn requests(&self) -> Vec<Value> {
        fs::read_to_string(&self.trace)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|frame| frame["command"].is_string())
            .collect()
    }
}

#[cfg(unix)]
#[test]
fn ssh_full_arguments_bypass_recognition_and_keep_paths_on_the_source() {
    let ssh = SshFixture::new();
    let relative = "server ' 中文 $()";
    let target = ssh.source.path().join(relative);
    fs::create_dir(&target).unwrap();
    fs::create_dir_all(ssh.client.path().join(relative).join(".mcpm")).unwrap();
    fs::write(
        ssh.client.path().join(relative).join(".mcpm/server.json"),
        b"client",
    )
    .unwrap();
    let output = ssh.run(&[
        relative,
        "--minecraft-version",
        "1.21.1",
        "--loader",
        "vanilla",
    ]);
    assert!(output.status.success(), "{output:?}");
    assert!(target.join(".mcpm/server.json").is_file());
    assert_eq!(
        fs::read(ssh.client.path().join(relative).join(".mcpm/server.json")).unwrap(),
        b"client"
    );
    let requests = ssh.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0]["command"], "server.init");
    assert_eq!(requests[0]["contents"]["path"], relative);
    assert_eq!(requests[0]["contents"]["metadata"]["loader"], Value::Null);
    assert_eq!(requests[0]["contents"]["metadata"]["version"], 1);
}

#[cfg(unix)]
#[test]
fn ssh_recognizes_default_source_script_then_initializes_and_raw_ambiguity_never_writes() {
    let ssh = SshFixture::new();
    fixture("forge", &ssh.source.path().join("server"));
    let output = ssh.run(&["server"]);
    assert!(output.status.success(), "{output:?}");
    let requests = ssh.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0]["command"], "server.init.recognize");
    assert!(
        requests[0]["contents"]
            .get("start_script")
            .is_none_or(Value::is_null)
    );
    assert_eq!(requests[1]["command"], "server.init");
    let start = &requests[1]["contents"]["metadata"]["start"];
    assert_eq!(start["script"], "run.sh");
    assert!(start.get("directory").is_none());
    assert_eq!(start["windows"], false);

    fixture("multiple", &ssh.source.path().join("multiple"));
    let output = ssh.run(&["multiple"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("multiple identified server launches")
    );
    assert!(!ssh.source.path().join("multiple/.mcpm").exists());
    let requests = ssh.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0]["command"], "server.init.recognize");

    // Partial values filter the recognized candidates before the same complete write command.
    let output = ssh.run(&["multiple", "--minecraft-version", "1.21.1"]);
    assert!(output.status.success(), "{output:?}");
    let requests = ssh.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(
        requests[1]["contents"]["metadata"]["minecraft_version"],
        "1.21.1"
    );
}
