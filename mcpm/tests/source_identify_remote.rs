//! Source-identification CLI ordering, command-owned validation and persistent-session interoperability.

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

#[test]
fn source_and_list_requests_share_one_server_and_preserve_file_failures() {
    let root = tempfile::tempdir().unwrap();
    let sample = root.path().join("sample.bin");
    std::fs::write(&sample, b" \tabc\r\n").unwrap();
    let paths = [
        root.path().join("missing-a.jar"),
        root.path().join("missing-b.jar"),
        root.path().join("missing-a.jar"),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("remote")
        .env("HOME", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let ready: Value = serde_json::from_str(&line).unwrap();
    assert!(
        ready["contents"]["commands"]
            .as_array()
            .unwrap()
            .contains(&json!("mod.source.identify"))
    );
    for (guid, command, contents) in [
        (
            1,
            "mod.source.identify",
            json!({"paths":paths,"requirements":{"murmur2":true},"parallelism":2}),
        ),
        (2, "mod.list", json!({"paths":[root.path()]})),
        (
            3,
            "mod.source.identify",
            json!({"paths":paths,"requirements":{"murmur2":false},"parallelism":0}),
        ),
        (
            4,
            "mod.source.identify",
            json!({"paths":[sample],"requirements":{"murmur2":true},"parallelism":1}),
        ),
    ] {
        writeln!(
            input,
            "{}",
            json!({"guid":uuid::Uuid::from_u128(guid),"command":command,"contents":contents})
        )
        .unwrap();
    }
    drop(input);
    let mut completed = 0;
    let mut seen = [false; 3];
    loop {
        line.clear();
        assert!(output.read_line(&mut line).unwrap() > 0);
        let reply: Value = serde_json::from_str(&line).unwrap();
        let id = uuid::Uuid::parse_str(reply["guid"].as_str().unwrap())
            .unwrap()
            .as_u128();
        if id == 0 {
            assert_eq!(reply["status"], "closed");
            break;
        }
        if reply["status"] == "finished" || reply["status"] == "error" {
            completed += 1;
        }
        if id == 1 && reply["contents"]["event"] == "item" {
            let item = &reply["contents"];
            assert_eq!(item["outcome"]["status"], "failed");
            assert!(
                !item["outcome"]["error"]["causes"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            for index in item["indices"].as_array().unwrap() {
                let state = &mut seen[index.as_u64().unwrap() as usize];
                assert!(!*state);
                *state = true;
            }
        }
        if id == 1 && reply["contents"]["event"] == "done" {
            assert_eq!(reply["contents"]["failed"], 3);
        }
        if id == 4 && reply["contents"]["event"] == "item" {
            assert_eq!(
                reply["contents"]["outcome"],
                json!({"status":"summary","summary":{"size":7,"sha1":"1b1c00108dba1baa64c120ae3aee9223109ad0e9","murmur2":1621425345}})
            );
        }
        if id == 3 {
            assert_eq!(reply["status"], "error");
        }
    }
    assert_eq!(completed, 4);
    assert!(seen.into_iter().all(|done| done));
    assert!(child.wait_with_output().unwrap().status.success());
}

#[cfg(unix)]
mod transport {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    fn script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fn print(value: Value, guid: &str) -> String {
        let line = format!("{value}\n").replace("__GUID__", "%s");
        format!("printf {} {guid}\n", shlex::try_quote(&line).unwrap())
    }
    fn setup(root: &Path, events: Vec<Value>) -> String {
        let ssh = root.join("ssh");
        script(
            &ssh,
            &format!(
                "printf 'connection\\n' >> {}\nfor last do :; done\nexec /bin/sh -c \"$last\"",
                shlex::try_quote(root.join("connections").to_str().unwrap()).unwrap()
            ),
        );
        let mut body = print(
            json!({"guid":uuid::Uuid::nil(),"status":"ready","contents":{"version":4,"commands":["mod.source.identify"],"max_concurrency":8}}),
            "",
        );
        body.push_str("IFS= read -r logs\nlog_guid=$(printf '%s' \"$logs\" | cut -d '\"' -f6)\n");
        body.push_str(&print(
            json!({"guid":"__GUID__","status":"ready","contents":null}),
            "\"$log_guid\"",
        ));
        body.push_str(&format!("IFS= read -r request\nprintf '%s' \"$request\" > {}\nguid=$(printf '%s' \"$request\" | cut -d '\"' -f4)\n", shlex::try_quote(root.join("request").to_str().unwrap()).unwrap()));
        for event in events {
            body.push_str(&print(
                json!({"guid":"__GUID__","status":"data","contents":event}),
                "\"$guid\"",
            ));
        }
        body.push_str(&print(
            json!({"guid":"__GUID__","status":"finished","contents":null}),
            "\"$guid\"",
        ));
        body.push_str("cat >/dev/null\n");
        body.push_str(&print(
            json!({"guid":"__GUID__","status":"finished","contents":null}),
            "\"$log_guid\"",
        ));
        body.push_str(&print(
            json!({"guid":uuid::Uuid::nil(),"status":"closed","contents":null}),
            "",
        ));
        script(&root.join("remote"), &body);
        format!(
            "{} user@fake",
            shlex::try_quote(ssh.to_str().unwrap()).unwrap()
        )
    }

    #[test]
    fn out_of_order_results_keep_input_order_and_remote_errors_keep_their_causes() {
        let root = tempfile::tempdir().unwrap();
        let connection = setup(
            root.path(),
            vec![
                json!({"event":"loading","indices":[0,3]}),
                json!({"event":"loading","indices":[1]}),
                json!({"event":"loading","indices":[2]}),
                json!({"event":"item","indices":[2],"outcome":{"status":"failed","error":{"message":"fast.jar","causes":["read failed"]}}}),
                json!({"event":"item","indices":[1],"outcome":{"status":"failed","error":{"message":"bad.jar","causes":["read failed","access denied"]}}}),
                json!({"event":"item","indices":[0,3],"outcome":{"status":"failed","error":{"message":"slow.jar","causes":["read failed"]}}}),
                json!({"event":"done","failed":4}),
            ],
        );
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .args([
                "--raw",
                "--log-level",
                "off",
                "--ssh",
                &connection,
                "--ssh-mcpm",
            ])
            .arg(root.path().join("remote"))
            .args([
                "mod",
                "source",
                "identify",
                "--source",
                "curseforge@PRIVATE-KEY",
                "--parallelism",
                "2",
                "--columns",
                "index,path,project,id,role,status",
                r"D:\slow.jar",
                "bad.jar",
                "fast.jar",
                r"D:\slow.jar",
            ])
            .env("HOME", root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains(
                "remote error: bad.jar\n  caused by: read failed\n  caused by: access denied\n"
            ),
            "{error}"
        );
        let paths: Vec<_> = error
            .lines()
            .filter(|line| {
                matches!(
                    *line,
                    "remote error: slow.jar" | "remote error: bad.jar" | "remote error: fast.jar"
                )
            })
            .collect();
        assert_eq!(
            paths,
            [
                "remote error: slow.jar",
                "remote error: bad.jar",
                "remote error: fast.jar",
                "remote error: slow.jar"
            ]
        );
        let request: Value =
            serde_json::from_str(&fs::read_to_string(root.path().join("request")).unwrap())
                .unwrap();
        assert_eq!(request["command"], "mod.source.identify");
        assert_eq!(
            request["contents"],
            json!({"paths":[r"D:\slow.jar","bad.jar","fast.jar",r"D:\slow.jar"],"requirements":{"murmur2":true},"parallelism":2,"cache":{"kind":"default"}})
        );
        assert_eq!(
            fs::read_to_string(root.path().join("connections"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn rejects_duplicate_missing_or_out_of_range_business_results() {
        for events in [
            vec![json!({"event":"loading","indices":[0,0]})],
            vec![json!({"event":"loading","indices":[1]})],
            vec![json!({"event":"done","failed":0})],
            vec![json!({"event":"loading","indices":[0]})],
        ] {
            let root = tempfile::tempdir().unwrap();
            let connection = setup(root.path(), events);
            let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
                .args(["--raw", "--ssh", &connection, "--ssh-mcpm"])
                .arg(root.path().join("remote"))
                .args([
                    "mod", "source", "identify", "--source", "modrinth", "one.jar",
                ])
                .env("HOME", root.path())
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
        }
    }
}
