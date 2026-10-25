//! Remote selection semantics for nested archives, cached failures and SSH frontend parity.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};

fn fixture(root: &Path) {
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        root.join("quoted ' mod.jar.disabled"),
    )
    .unwrap();
    fs::write(root.join("broken.jar"), b"invalid archive").unwrap();
}

fn remote(root: &Path, command: &str, contents: Value) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
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
    let replies = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies[replies.len() - 2]["status"], "finished");
    replies
        .into_iter()
        .filter(|reply| reply["status"] == "data")
        .map(|reply| reply["contents"].clone())
        .collect()
}

#[test]
fn nested_queries_preserve_archive_subtrees_order_and_deduplicated_failure_counts() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    for command in ["mod.info", "mod.deps"] {
        let events = remote(
            root.path(),
            command,
            json!({"parallelism":3,"loaders":["fabric"],"selection":{"queries":[
                {"kind":"modid","value":"omnibus_nested"},
                {"kind":"file","value":"missing.jar"},
                {"kind":"file","value":"missing.jar"},
                {"kind":"file","value":"quoted ' mod.jar.disabled"}
            ]}}),
        );
        // Index scans report all completions before any selected subtree is emitted.
        // Repeated missing files and cached archive selections do not trigger new scans.
        let progress = events
            .iter()
            .filter(|event| event["event"] == "scan")
            .map(|event| {
                (
                    event["completed"].as_u64().unwrap(),
                    event["total"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(progress, [(0, 2), (1, 2), (2, 2), (0, 1), (1, 1)]);
        let first_file = events
            .iter()
            .position(|event| event["event"] == "file")
            .unwrap();
        assert_eq!(
            events[..first_file]
                .iter()
                .filter(|event| event["event"] == "scan")
                .count(),
            3
        );
        let files = events
            .iter()
            .filter(|event| event["event"] == "file")
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 4, "{events:?}");
        assert_eq!(files[0]["index"], 0);
        assert_eq!(files[3]["index"], 3);
        let declarations = files[0]["file"]["metadata"][0]["records"]
            .as_array()
            .unwrap();
        let ids = declarations
            .iter()
            .map(|record| record["info"]["modid"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["omnibus_nested", "omnibus_leaf"]);
        assert_eq!(
            declarations[1]["source"],
            json!(["META-INF/jars/child.jar", "META-INF/jars/grandchild.jar"])
        );
        assert_eq!(files[3]["file"]["filename"], "quoted ' mod.jar.disabled");
        assert_eq!(files[3]["file"]["enabled"], false);
        // Building the name/id index also encounters broken.jar; repeated missing inputs share a failure.
        assert_eq!(
            events.last().unwrap(),
            &json!({"event":"done","files":4,"failed":2,"ignored":0,"cache":{"fully_cached":0,"partially_cached":0,"total":3}})
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event"] == "error")
                .count(),
            2
        );
    }
    for args in [
        json!({"selection":{"all":true,"queries":[{"kind":"file","value":"a.jar"}]}}),
        json!({"loaders":["unknown"]}),
        json!({"parallelism":0}),
        json!({"selection":{"queries":[{"kind":"invalid","value":"a.jar"}]}}),
    ] {
        let events = remote(root.path(), "mod.info", args);
        assert_eq!(events[0]["event"], "error");
        assert_eq!(
            events[1],
            json!({"event":"done","files":0,"failed":1,"ignored":0})
        );
    }
}

#[cfg(unix)]
#[test]
fn ssh_resolves_queries_on_remote_host_and_preserves_local_presentation() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::Builder::new()
        .prefix("mcpm inspect '")
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
    let run = |command: &str, options: &[&str], over_ssh: bool, stdin: &str| -> Output {
        let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
        process
            .args([
                "--raw",
                "--remote-logs",
                "--log-level",
                "off",
                "mod",
                command,
            ])
            .args(options);
        if command == "info" {
            process.arg("--no-cache");
        }
        if over_ssh {
            process.args([
                "--ssh",
                &connection,
                "--ssh-mcpm",
                env!("CARGO_BIN_EXE_mcpm-tools"),
            ]);
        } else {
            process.current_dir(root.path());
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
                "--loader",
                "fabric",
            ],
            vec!["--all", "--loader", "forge", "--parallelism", "2"],
        ] {
            let stdin = if options.contains(&"--all") {
                ""
            } else {
                "quoted ' mod.jar.disabled\n"
            };
            let local = run(command, &options, false, stdin);
            let remote = run(command, &options, true, stdin);
            assert_eq!(local.status.code(), remote.status.code(), "{remote:?}");
            assert_eq!(local.stdout, remote.stdout);
            assert_eq!(
                local.stderr,
                String::from_utf8(remote.stderr)
                    .unwrap()
                    .replace("remote error:", "error:")
                    .as_bytes()
            );
        }
    }
    fs::remove_file(root.path().join("broken.jar")).unwrap();
    for style in ["inner", "outer", "full"] {
        let options = ["--modid", "omnibus_nested", "--path-style", style];
        let local = run("deps", &options, false, "");
        let remote = run("deps", &options, true, "");
        assert!(remote.status.success(), "{remote:?}");
        assert_eq!(local.stdout, remote.stdout);
    }
}
