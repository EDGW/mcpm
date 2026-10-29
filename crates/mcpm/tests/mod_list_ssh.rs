//! CLI coverage for SSH option scope, typed parameters and remote protocol failures.

use std::process::{Command, Stdio};

/// Build identity both ends must agree on; read from the executable under test.
fn build_version() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .arg("--version")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    stdout
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned()
}

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
        let executable = root.path().join("mcpm-tools");
        symlink(env!("CARGO_BIN_EXE_mcpm-tools"), &executable).unwrap();
        let windows_executable = root.path().join("mcpm-tools.exe");
        symlink(env!("CARGO_BIN_EXE_mcpm-tools"), &windows_executable).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
        let quick = root.path().join("quick.jar.disabled");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/dependency-problems/mods/fabric_only.jar"),
            &quick,
        )
        .unwrap();
        let options = [
            "--no-cache",
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
        let local = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
            .args(["--raw", "mods", "list"])
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
            let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
            command
                .args(["--raw", "--ssh", &connection, "mods", "list", "--no-ui"])
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
            assert_eq!(
                output.stdout,
                local.stdout,
                "remote stderr: {:?}",
                String::from_utf8_lossy(&output.stderr)
            );
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
        let command_id = "mods.jar.read";
        let ready = format!(
            "{{\"guid\":\"{nil}\",\"status\":\"ready\",\"contents\":{{\"version\":6,\"build\":\"{build}\",\"operations\":[\"{command_id}\"]}}}}\n",
            build = build_version()
        );
        for (body, status, message) in [
            (
                "printf '{\"guid\":\"%s\",\"status\":\"data\",\"contents\":{}}' \"$guid\"",
                0,
                "newline",
            ),
            (
                "printf '%s\n' '{\"guid\":\"00000000-0000-0000-0000-000000000099\",\"status\":\"data\",\"contents\":{}}'",
                0,
                "protocol",
            ),
            (
                "printf '%s\n' '{\"guid\":\"00000000-0000-0000-0000-000000000000\",\"status\":\"closed\",\"contents\":null}'",
                255,
                "protocol",
            ),
        ] {
            fs::write(&remote, format!(
                "#!/bin/sh\nprintf '%s' {}\nIFS= read -r request\nguid=$(printf '%s' \"$request\" | cut -d '\"' -f4)\n{body}\nexit {status}\n",
                shlex::try_quote(&ready).unwrap()
            )).unwrap();
            fs::set_permissions(&remote, fs::Permissions::from_mode(0o700)).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
                .args(["--raw", "mods", "list", "--no-cache", "--ssh", &connection, "--ssh-mcpm"])
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
    fn requests_are_typed_and_remote_records_keep_their_order() {
        use serde_json::{Value, json};
        fn print(value: Value, guid: &str) -> String {
            let line = format!("{value}\n").replace("__GUID__", "%s");
            format!("printf {} {guid}\n", shlex::try_quote(&line).unwrap())
        }
        let item = |index, path: &str| {
            json!({"event":"item","index":index,"record":{
                "path":path,"name":path.trim_end_matches(".jar"),"enabled":true,
                "status":"unrecognized","metadata":[],"errors":[]
            }})
        };
        let root = tempfile::tempdir().unwrap();
        let connection = setup(root.path());
        let remote = root.path().join("remote");
        let mut body = String::from("#!/bin/sh\n");
        body += &print(
            json!({"guid":uuid::Uuid::nil(),"status":"ready","contents":{"version":6,"build":build_version(),"operations":["mods.jar.read"]}}),
            "",
        );
        body += &format!(
            "IFS= read -r request\nprintf '%s' \"$request\" > {}\nguid=$(printf '%s' \"$request\" | cut -d '\"' -f4)\n",
            shlex::try_quote(root.path().join("request").to_str().unwrap()).unwrap()
        );
        body += &print(
            json!({"guid":"__GUID__","status":"data","contents":{
                "event":"scope","loaders":[],"environment":{"cache":null}
            }}),
            "\"$guid\"",
        );
        body += &print(
            json!({"guid":"__GUID__","status":"data","contents":item(0, "z.jar")}),
            "\"$guid\"",
        );
        body += &print(
            json!({"guid":"__GUID__","status":"data","contents":item(1, "a.jar")}),
            "\"$guid\"",
        );
        body += &print(
            json!({"guid":"__GUID__","status":"finished","contents":{
                "files":2,"input_errors":0,"failed":0,"cache":null
            }}),
            "\"$guid\"",
        );
        body += "cat >/dev/null\n";
        body += &print(
            json!({"guid":uuid::Uuid::nil(),"status":"closed","contents":null}),
            "",
        );
        fs::write(&remote, body).unwrap();
        fs::set_permissions(&remote, fs::Permissions::from_mode(0o700)).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
            .args(["--raw", "--ssh", &connection, "--ssh-mcpm"])
            .arg(&remote)
            .args([
                "mods",
                "list",
                "--no-cache",
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
            serde_json::from_str(&fs::read_to_string(root.path().join("request")).unwrap()).unwrap();
        assert_eq!(request["operation"], "mods.jar.read");
        assert_eq!(request["input"]["parallelism"], 2);
        assert_eq!(
            request["input"]["scope"],
            json!({"kind":"paths","directory":".","paths":["z.jar","a.jar"],"loaders":[]})
        );
        assert!(request["input"].get("identify_parallelism").is_none());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stderr}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.find("z.jar").unwrap() < stdout.find("a.jar").unwrap(),
            "{stdout}"
        );
    }
}
