//! Server-scoped remote inspection: loader ownership, archive selections, failures and SSH parity.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

const ARCHIVE: &str = "quoted ' 中文 mod.jar.disabled";

fn fixture(server: &Path, loader: &str) {
    fs::create_dir_all(server.join(".mcpm")).unwrap();
    fs::create_dir_all(server.join("mods")).unwrap();
    fs::write(
        server.join(".mcpm/server.json"),
        serde_json::to_vec(&json!({
            "version":1, "minecraft_version":"1.21.1",
            "loader":{"type":loader,"version":"1.0"}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        server.join("mods").join(ARCHIVE),
    )
    .unwrap();
    fs::write(server.join("mods/broken.jar"), b"invalid archive").unwrap();
}

fn remote(root: &Path, command: &str, contents: Value) -> Vec<Value> {
    fs::create_dir_all(root.join("home")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .env("HOME", root.join("home"))
        .arg("remote")
        .current_dir(root)
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

#[test]
fn selections_use_server_loader_and_scoped_paths_with_cached_failures() {
    let root = tempfile::tempdir().unwrap();
    let server = root.path().join("server ' 中文");
    fixture(&server, "fabric");
    for command in ["server.mods.info", "server.mods.deps"] {
        let replies = remote(
            root.path(),
            command,
            json!({"server":"server ' 中文", "no_cache":true, "parallelism":3, "selection":{"queries":[
                {"kind":"modid","value":"omnibus_nested"},
                {"kind":"file","value":"missing.jar"},
                {"kind":"file","value":"missing.jar"},
                {"kind":"file","value":"../outside.jar"},
                {"kind":"file","value":format!("mods/{ARCHIVE}")},
                {"kind":"file","value":ARCHIVE}
            ]}}),
        );
        assert_eq!(replies[replies.len() - 2]["status"], "finished");
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
        // The lazy index is complete before selecting a nested archive. Repeated
        // files share the scan cache, including the missing-file failure.
        assert_eq!(progress, [(0, 2), (1, 2), (2, 2), (0, 1), (1, 1)]);
        let files: Vec<_> = events
            .iter()
            .filter(|event| event["event"] == "file")
            .collect();
        assert_eq!(files.len(), 5, "{events:?}");
        for (index, event) in files.iter().enumerate() {
            assert_eq!(event["index"], index);
        }
        for index in [0, 3, 4] {
            let event = files[index];
            assert_eq!(event["file"]["filename"], ARCHIVE);
            assert_eq!(event["file"]["enabled"], false);
            let metadata = event["file"]["metadata"].as_array().unwrap();
            // The fixture advertises three loaders; only the server's loader applies.
            assert_eq!(metadata.len(), 1);
            assert_eq!(metadata[0]["loader"], "fabric");
        }
        let records = files[0]["file"]["metadata"][0]["records"]
            .as_array()
            .unwrap();
        assert_eq!(
            records
                .iter()
                .map(|record| record["info"]["modid"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["omnibus_nested", "omnibus_leaf"]
        );
        assert_eq!(
            records[1]["source"],
            json!(["META-INF/jars/child.jar", "META-INF/jars/grandchild.jar"])
        );
        assert_eq!(files[1]["file"], files[2]["file"]);
        assert_eq!(files[1]["file"]["failed_loaders"], json!(["fabric"]));
        assert_eq!(files[1]["file"]["metadata"], json!([]));
        assert_eq!(files[3]["file"], files[4]["file"]);
        assert!(
            files[3]["file"]["metadata"][0]["records"]
                .as_array()
                .unwrap()
                .len()
                > records.len()
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event"] == "error")
                .count(),
            3
        );
        assert_eq!(
            **events.last().unwrap(),
            json!({"event":"done","files":5,"failed":3,"ignored":0,"cache":{"fully_cached":0,"partially_cached":0,"total":3}})
        );
    }
}

#[test]
fn all_reports_progress_while_invalid_server_or_request_fails_without_completion() {
    let root = tempfile::tempdir().unwrap();
    fixture(&root.path().join("forge"), "forge");
    fixture(&root.path().join("unsupported"), "unknown-loader");
    fixture(&root.path().join("invalid"), "fabric");
    fs::write(
        root.path().join("invalid/.mcpm/server.json"),
        b"invalid metadata",
    )
    .unwrap();
    for command in ["server.mods.info", "server.mods.deps"] {
        let replies = remote(
            root.path(),
            command,
            json!({"server":"forge","parallelism":2,"selection":{"all":true}}),
        );
        let events: Vec<_> = replies
            .iter()
            .filter(|reply| reply["status"] == "data")
            .map(|reply| &reply["contents"])
            .collect();
        let progress: Vec<_> = events
            .iter()
            .filter(|event| event["event"] == "scan")
            .map(|event| event["completed"].as_u64().unwrap())
            .collect();
        assert_eq!(progress, [0, 1, 2]);
        let file = events
            .iter()
            .find(|event| {
                event["event"] == "file"
                    && !event["file"]["metadata"].as_array().unwrap().is_empty()
            })
            .unwrap();
        assert_eq!(file["file"]["metadata"].as_array().unwrap().len(), 1);
        assert_eq!(file["file"]["metadata"][0]["loader"], "forge");
        assert_eq!(
            **events.last().unwrap(),
            json!({"event":"done","files":2,"failed":1,"ignored":0,"cache":{"fully_cached":0,"partially_cached":0,"total":2}})
        );
        for contents in [
            json!({"server":"unsupported","selection":{"all":true}}),
            json!({"server":"invalid","selection":{"all":true}}),
            json!({"server":"missing","selection":{"all":true}}),
            json!({"server":"forge","parallelism":0,"selection":{"all":true}}),
            json!({"server":"forge","selection":{"all":true,"queries":[{"kind":"file","value":ARCHIVE}]}}),
        ] {
            let replies = remote(root.path(), command, contents);
            assert!(
                replies.iter().any(|reply| reply["status"] == "error"
                    && reply["contents"]["code"] == "handler_failed"),
                "{replies:?}"
            );
            assert!(
                !replies
                    .iter()
                    .any(|reply| matches!(reply["status"].as_str(), Some("data" | "finished"))),
                "{replies:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn ssh_resolves_server_and_stdin_on_source_and_matches_local_output() {
    use std::os::unix::fs::PermissionsExt;

    let source = tempfile::Builder::new()
        .prefix("mcpm source ' 中文 ")
        .tempdir()
        .unwrap();
    let client = tempfile::tempdir().unwrap();
    let server = "server ' 中文";
    fixture(&source.path().join(server), "fabric");
    // Same-name client metadata cannot influence the remote server's scope.
    fs::create_dir_all(client.path().join(server).join(".mcpm")).unwrap();
    fs::write(
        client.path().join(server).join(".mcpm/server.json"),
        b"invalid client metadata",
    )
    .unwrap();
    let ssh = source.path().join("ssh");
    fs::write(
        &ssh,
        format!(
            "#!/bin/sh\ncd {} || exit 1\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
            shlex::try_quote(source.path().to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    let connection = shlex::try_join([ssh.to_str().unwrap(), "test@host"]).unwrap();
    let run = |command: &str, options: &[&str], over_ssh: bool, stdin: &str| {
        let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm"));
        process
            .args([
                "--raw",
                "--remote-logs",
                "--log-level",
                "off",
                "server",
                "mods",
                command,
                "--server",
                server,
            ])
            .args(options);
        if command == "info" {
            process.arg("--no-cache");
        }
        if over_ssh {
            process.current_dir(client.path()).args([
                "--ssh",
                &connection,
                "--ssh-mcpm",
                env!("CARGO_BIN_EXE_mcpm"),
            ]);
        } else {
            process.current_dir(source.path());
        }
        let mut child = process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    for command in ["info", "deps"] {
        for options in [
            vec![
                "--modid",
                "omnibus_nested",
                "--file",
                "missing.jar",
                "--input",
                "file",
            ],
            vec!["--all", "--parallelism", "2"],
        ] {
            let stdin = if options.contains(&"--all") {
                String::new()
            } else {
                format!("mods/{ARCHIVE}\n")
            };
            let local = run(command, &options, false, &stdin);
            let remote = run(command, &options, true, &stdin);
            assert_eq!(local.status.code(), Some(1), "{local:?}");
            assert_eq!(remote.status.code(), local.status.code(), "{remote:?}");
            assert_eq!(remote.stdout, local.stdout);
            assert_eq!(
                String::from_utf8(remote.stderr)
                    .unwrap()
                    .replace("remote error:", "error:")
                    .as_bytes(),
                local.stderr
            );
        }
    }
    fs::remove_file(source.path().join(server).join("mods/broken.jar")).unwrap();
    for style in ["inner", "outer", "full"] {
        let options = ["--modid", "omnibus_nested", "--path-style", style];
        let local = run("deps", &options, false, "");
        let remote = run("deps", &options, true, "");
        assert!(remote.status.success(), "{remote:?}");
        assert_eq!(remote.stdout, local.stdout);
        assert!(remote.stderr.is_empty(), "{remote:?}");
    }
}
