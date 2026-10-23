//! CLI coverage for SSH option scope, typed parameters and remote protocol failures.

use std::process::{Command, Stdio};

#[cfg(unix)]
mod transport {
    use super::*;
    use std::{
        fs,
        io::Write,
        os::unix::fs::{PermissionsExt, symlink},
        path::Path,
    };

    fn setup(root: &Path) -> String {
        let ssh = root.join("ssh");
        fs::write(
            &ssh,
            format!(
                "#!/bin/sh\nprintf 'connection\\n' >> {}\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
                shlex::try_quote(root.join("connections").to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
        shlex::try_join([ssh.to_str().unwrap(), "-i", "key with spaces", "test@host"]).unwrap()
    }

    #[test]
    fn keeps_quoted_filters_and_display_options_local_and_sends_stdin_paths() {
        let root = tempfile::Builder::new()
            .prefix("mcpm ssh '& 中文 ")
            .tempdir()
            .unwrap();
        let connection = setup(root.path());
        let executable = root.path().join("mcpm");
        symlink(env!("CARGO_BIN_EXE_mcpm"), &executable).unwrap();
        let windows_executable = root.path().join("mcpm.exe");
        symlink(env!("CARGO_BIN_EXE_mcpm"), &windows_executable).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
        let quick = root.path().join("quick.jar.disabled");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/dependency-problems/mods/fabric_only.jar"),
            &quick,
        )
        .unwrap();
        let options = [
            "--loader",
            "fabric",
            "--expanded",
            "--index-type",
            "tree",
            "--path-style",
            "inner",
            "--filter",
            "@name \"Nested Module\" | @modid =omnibus_leaf",
            "--width",
            "40",
            "-C",
            "IMF",
            "--parallelism",
            "3",
            "--identify-parallelism",
            "2",
        ];
        let local = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .args(["--raw", "mod", "list"])
            .arg(&fixture)
            .arg(&quick)
            .arg(&fixture)
            .args(options)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(local.status.success(), "{:?}", local);
        for (index, remote_executable) in [None, Some(&executable), Some(&windows_executable)]
            .into_iter()
            .enumerate()
        {
            let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm"));
            command
                .args(["--raw", "--ssh", &connection, "mod", "list", "--no-ui"])
                .args(options)
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        root.path().display(),
                        std::env::var("PATH").unwrap()
                    ),
                );
            if let Some(remote_executable) = remote_executable {
                command.arg("--ssh-mcpm").arg(remote_executable);
            }
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            writeln!(
                child.stdin.take().unwrap(),
                "{}\n{}\n{}",
                fixture.display(),
                quick.display(),
                fixture.display()
            )
            .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{:?}", output);
            assert!(output.stderr.is_empty(), "{:?}", output);
            assert_eq!(output.stdout, local.stdout);
            let connections = fs::read_to_string(root.path().join("connections")).unwrap();
            assert_eq!(connections.lines().count(), index + 1);
        }
    }

    #[test]
    fn rejects_corrupted_remote_streams_and_propagates_transport_exit_status() {
        let root = tempfile::tempdir().unwrap();
        let connection = setup(root.path());
        let remote = root.path().join("remote");
        let nil = "00000000-0000-0000-0000-000000000000";
        let command_id = "mod.list";
        let ready = format!(
            "{{\"guid\":\"{nil}\",\"status\":\"ready\",\"contents\":{{\"version\":3,\"commands\":[\"{command_id}\"],\"max_concurrency\":8}}}}\n"
        );
        for (body, status, message) in [
            (
                "printf '{\"guid\":\"%s\",\"status\":\"data\",\"contents\":{\"event\":\"count\",\"count\":1}}\n{\"guid\":\"%s\",\"status\":\"data\",\"contents\":{\"event\":\"done\",\"success\":true}}\n{\"guid\":\"%s\",\"status\":\"finished\",\"contents\":null}\n' \"$guid\" \"$guid\" \"$guid\"",
                0, "count",
            ),
            ("printf '{\"guid\":\"%s\",\"status\":\"data\",\"contents\":{}}' \"$guid\"", 0, "newline"),
            ("printf '%s\n' '{\"guid\":\"00000000-0000-0000-0000-000000000099\",\"status\":\"data\",\"contents\":{}}'", 0, "request"),
            (
                "printf '{\"guid\":\"%s\",\"status\":\"data\",\"contents\":{\"event\":\"count\",\"count\":0}}\n{\"guid\":\"%s\",\"status\":\"data\",\"contents\":{\"event\":\"done\",\"success\":true}}\n{\"guid\":\"%s\",\"status\":\"finished\",\"contents\":null}\n' \"$guid\" \"$guid\" \"$guid\"
cat >/dev/null
printf '%s\n' '{\"guid\":\"00000000-0000-0000-0000-000000000000\",\"status\":\"closed\",\"contents\":null}'",
                255, "255",
            ),
        ] {
            fs::write(&remote, format!(
                "#!/bin/sh\nprintf '%s' {}\nIFS= read -r logs\nlog_guid=$(printf '%s' \"$logs\" | cut -d '\"' -f6)\nprintf '{{\"guid\":\"%s\",\"status\":\"ready\",\"contents\":null}}\\n' \"$log_guid\"\nIFS= read -r request\nguid=$(printf '%s' \"$request\" | cut -d '\"' -f4)\n{body}\nexit {status}\n",
                shlex::try_quote(&ready).unwrap()
            )).unwrap();
            fs::set_permissions(&remote, fs::Permissions::from_mode(0o700)).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
                .args(["--raw", "mod", "list", "--ssh", &connection, "--ssh-mcpm"])
                .arg(&remote)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{:?}", output);
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(message),
                "{:?}",
                output
            );
        }
    }
    #[test]
    fn parallel_results_are_reordered_and_invalid_indices_or_lifecycles_are_rejected() {
        use serde_json::{Value, json};
        fn print(value: Value, guid: &str) -> String {
            let line = format!("{value}\n").replace("__GUID__", "%s");
            format!("printf {} {guid}\n", shlex::try_quote(&line).unwrap())
        }
        let count = json!({"event":"count","count":2});
        let done = json!({"event":"done","success":true});
        let item = |index, path: &str| json!({"event":"item","index":index,"record":{"path":path,"name":path.trim_end_matches(".jar"),"enabled":true,"status":"unrecognized","metadata":[],"errors":[]}});
        for (events, expected) in [
            (
                vec![
                    count.clone(),
                    item(1, "a.jar"),
                    item(0, "z.jar"),
                    done.clone(),
                ],
                None,
            ),
            (
                vec![
                    count.clone(),
                    item(0, "z.jar"),
                    item(0, "z.jar"),
                    done.clone(),
                ],
                Some("index/count"),
            ),
            (
                vec![count.clone(), item(1, "a.jar"), done.clone()],
                Some("count differs"),
            ),
            (
                vec![count.clone(), item(2, "a.jar"), done.clone()],
                Some("index/count"),
            ),
            (
                vec![item(0, "z.jar"), count.clone(), done.clone()],
                Some("before its count"),
            ),
            (
                vec![count.clone(), done.clone(), item(0, "z.jar")],
                Some("after completion"),
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let connection = setup(root.path());
            let remote = root.path().join("remote");
            let mut body = String::from("#!/bin/sh\n");
            body += &print(
                json!({"guid":uuid::Uuid::nil(),"status":"ready","contents":{"version":3,"commands":["mod.list"],"max_concurrency":8}}),
                "",
            );
            body += "IFS= read -r logs\nlog_guid=$(printf '%s' \"$logs\" | cut -d '\"' -f6)\n";
            body += &print(
                json!({"guid":"__GUID__","status":"ready","contents":null}),
                "\"$log_guid\"",
            );
            body += &format!(
                "IFS= read -r request\nprintf '%s' \"$request\" > {}\nguid=$(printf '%s' \"$request\" | cut -d '\"' -f4)\n",
                shlex::try_quote(root.path().join("request").to_str().unwrap()).unwrap()
            );
            for event in events {
                body += &print(
                    json!({"guid":"__GUID__","status":"data","contents":event}),
                    "\"$guid\"",
                );
            }
            body += &print(
                json!({"guid":"__GUID__","status":"finished","contents":null}),
                "\"$guid\"",
            );
            body += "cat >/dev/null\n";
            body += &print(
                json!({"guid":"__GUID__","status":"finished","contents":null}),
                "\"$log_guid\"",
            );
            body += &print(
                json!({"guid":uuid::Uuid::nil(),"status":"closed","contents":null}),
                "",
            );
            fs::write(&remote, body).unwrap();
            fs::set_permissions(&remote, fs::Permissions::from_mode(0o700)).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
                .args(["--raw", "--ssh", &connection, "--ssh-mcpm"])
                .arg(&remote)
                .args([
                    "mod",
                    "list",
                            "z.jar",
                    "a.jar",
                    "--parallelism",
                    "2",
                    "--identify-parallelism",
                    "1",
                    "--columns",
                    "index,filename",
                ])
                .env("HOME", root.path())
                .stdin(Stdio::null())
                .output()
                .unwrap();
            let request: Value =
                serde_json::from_str(&fs::read_to_string(root.path().join("request")).unwrap())
                    .unwrap();
            assert_eq!(request["command"], "mod.list");
            assert_eq!(request["contents"]["parallelism"], 2);
            assert!(request["contents"].get("identify_parallelism").is_none());
            let stderr = String::from_utf8(output.stderr).unwrap();
            if let Some(expected) = expected {
                assert_eq!(output.status.code(), Some(1), "{stderr}");
                assert!(output.stdout.is_empty());
                assert!(stderr.contains(expected), "{stderr}");
            } else {
                assert!(output.status.success(), "{stderr}");
                let stdout = String::from_utf8(output.stdout).unwrap();
                assert!(
                    stdout.find("z.jar").unwrap() < stdout.find("a.jar").unwrap(),
                    "{stdout}"
                );
            }
        }
    }
}
