//! Server metadata transport across source-machine paths, startup platforms and SSH failures.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};

fn fixture(root: &Path) -> Value {
    fs::create_dir_all(root.join(".mcpm")).unwrap();
    let metadata = json!({
        "version":1,
        "minecraft_version":"1.21.1",
        "loader":{"type":"fabric","version":"0.16.10"},
        "start":{
            "script":"run.bat",
            "executable":"C:\\Program Files\\Java\\bin\\java.exe",
            "arguments":["", "-Dmessage=a \\\"quoted\\\" value", "C:\\server files\\", "-jar", "server.jar", "$(touch escaped)"],
            "windows":true
        }
    });
    fs::write(
        root.join(".mcpm/server.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    metadata
}

fn remote(root: &Path, contents: Value) -> Vec<Value> {
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
        json!({"guid":uuid::Uuid::from_u128(1),"command":"server.info","contents":contents})
    )
    .unwrap();
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
fn snapshot_retains_source_paths_loader_identity_and_windows_argv() {
    let temp = tempfile::Builder::new()
        .prefix("mcpm server ' 中文 ")
        .tempdir()
        .unwrap();
    let root = temp.path().join("服务端 ' $()");
    fs::create_dir(&root).unwrap();
    let metadata = fixture(&root);
    let replies = remote(temp.path(), json!({"path":"服务端 ' $()"}));
    let data = replies
        .iter()
        .filter(|reply| reply["status"] == "data")
        .collect::<Vec<_>>();
    assert_eq!(data.len(), 1, "{replies:?}");
    let snapshot = &data[0]["contents"];
    assert_eq!(snapshot["name"], "服务端 ' $()");
    assert_eq!(snapshot["path"], "服务端 ' $()");
    assert_eq!(snapshot["minecraft_version"], metadata["minecraft_version"]);
    assert_eq!(snapshot["loader"]["id"], "fabric");
    assert_eq!(snapshot["loader"]["name"], "Fabric");
    assert_eq!(snapshot["loader"]["version"], "0.16.10");
    assert_eq!(snapshot["start"]["directory"], "服务端 ' $()");
    for field in ["executable", "arguments", "windows"] {
        assert_eq!(snapshot["start"][field], metadata["start"][field]);
    }
    assert_eq!(replies[replies.len() - 2]["status"], "finished");
    assert_eq!(replies.last().unwrap()["status"], "closed");
    // Path ownership belongs to the handler; an invalid or missing server fails the invocation.
    for contents in [
        json!({"path":"missing server"}),
        json!({"path":"服务端 ' $()", "unexpected":true}),
    ] {
        let replies = remote(temp.path(), contents);
        assert!(!replies.iter().any(|reply| reply["status"] == "data"));
        assert!(replies.iter().any(|reply| {
            reply["status"] == "error" && reply["contents"]["code"] == "handler_failed"
        }));
        assert!(!replies.iter().any(|reply| reply["status"] == "finished"));
    }
}

#[cfg(unix)]
#[test]
fn ssh_owns_relative_paths_preserves_startup_platform_and_propagates_load_failures() {
    use std::os::unix::fs::PermissionsExt;

    let source = tempfile::Builder::new()
        .prefix("mcpm remote ' 中文 ")
        .tempdir()
        .unwrap();
    let client = tempfile::tempdir().unwrap();
    let relative = "server ' 中文 $()";
    let server = source.path().join(relative);
    fs::create_dir(&server).unwrap();
    fixture(&server);
    // A same-name client directory must not supply metadata for an SSH request.
    fs::create_dir_all(client.path().join(relative).join(".mcpm")).unwrap();
    fs::write(
        client.path().join(relative).join(".mcpm/server.json"),
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
    let run = |path: &str, over_ssh: bool| -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
        command.args([
            "--raw",
            "--log-level",
            "off",
            "server",
            "info",
            path,
            "--full-command",
        ]);
        if over_ssh {
            command.current_dir(client.path()).args([
                "--ssh",
                &connection,
                "--ssh-mcpm",
                env!("CARGO_BIN_EXE_mcpm-tools"),
            ]);
        } else {
            command.current_dir(source.path());
        }
        command.output().unwrap()
    };
    let local = run(relative, false);
    let remote = run(relative, true);
    assert!(remote.status.success(), "{remote:?}");
    assert_eq!(local.status.code(), remote.status.code());
    assert_eq!(local.stdout, remote.stdout);
    assert!(remote.stderr.is_empty(), "{remote:?}");
    let output = String::from_utf8(remote.stdout).unwrap();
    let command = output.trim_end().split('\t').next_back().unwrap();
    // Foreign Windows argv is quoted using Windows rules even on the Unix SSH frontend.
    assert!(command.starts_with("\"C:\\Program Files\\Java\\bin\\java.exe\" \"\" "));
    assert!(command.contains("\"C:\\server files\\\\\""));
    assert!(command.ends_with("\"$(touch escaped)\""));
    assert!(!source.path().join("escaped").exists());
    assert!(!client.path().join("escaped").exists());
    for path in [relative, "missing server"] {
        if path == relative {
            fs::write(server.join(".mcpm/server.json"), b"invalid source metadata").unwrap();
        }
        let local = run(path, false);
        let remote = run(path, true);
        assert_eq!(remote.status.code(), Some(1), "{remote:?}");
        assert_eq!(local.status.code(), remote.status.code());
        assert!(remote.stdout.is_empty(), "{remote:?}");
        let error = String::from_utf8(remote.stderr).unwrap();
        let local_error = String::from_utf8(local.stderr).unwrap();
        assert!(error.contains(path), "{error}");
        // The remote transport preserves the loader's error and its underlying cause chain.
        assert!(
            error.contains(local_error.trim().trim_start_matches("error: ")),
            "local: {local_error}\nremote: {error}"
        );
    }
}
