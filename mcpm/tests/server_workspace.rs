//! Remote workspace registration, automatic routing, precedence and Local-only endpoint boundaries.

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

fn local(root: &Path) {
    write(root, "mcpm.json", json!({"version":1,"type":"local"}));
    write(
        root,
        "server.json",
        json!({"version":2,"minecraft_version":"1.21.1","loader":{"type":"fabric","version":"0.16.10"}}),
    );
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .current_dir(root)
        .args(["--raw", "--remote-logs", "--log-level", "off"])
        .args(args)
        .output()
        .unwrap()
}

fn success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

fn failure(output: &Output, message: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
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
        ("server.info", json!({})),
        ("server.init.recognize", json!({})),
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
        (
            "server.init",
            json!({"metadata":{"version":1,"minecraft_version":"1.21.1","loader":null},"force":true}),
        ),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
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

#[cfg(unix)]
struct Fixture {
    root: tempfile::TempDir,
    connection: String,
}

#[cfg(unix)]
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::Builder::new()
            .prefix("mcpm workspace ' 中文 ")
            .tempdir()
            .unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir_all(workspace.join("keys")).unwrap();
        fs::write(workspace.join("keys/key"), "test credential").unwrap();
        let host = root.path().join("host");
        fs::create_dir(&host).unwrap();
        local(&host.join("server ' 中文"));
        let ssh = workspace.join("ssh");
        fs::write(&ssh, format!("#!/bin/sh\ntest -f keys/key || exit 91\npwd >> {}\ncd {} || exit 1\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
            shlex::try_quote(root.path().join("cwd.log").to_str().unwrap()).unwrap(),
            shlex::try_quote(host.to_str().unwrap()).unwrap())).unwrap();
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            connection: "./ssh -i keys/key test@host".into(),
        }
    }

    fn workspace(&self) -> std::path::PathBuf {
        self.root.path().join("workspace")
    }
    fn host(&self) -> std::path::PathBuf {
        self.root.path().join("host")
    }

    fn register(&self, extra: &[&str]) -> Output {
        let workspace = self.workspace();
        let mut args = vec![
            "--ssh",
            &self.connection,
            "--ssh-mcpm",
            env!("CARGO_BIN_EXE_mcpm"),
            "server",
            "init",
            workspace.to_str().unwrap(),
            "--remote",
            "server ' 中文",
        ];
        args.extend_from_slice(extra);
        run(self.root.path(), &args)
    }
}

#[cfg(unix)]
#[test]
fn remote_registration_is_read_only_then_routes_commands_from_any_client_directory() {
    let fixture = Fixture::new();
    let server = fixture.host().join("server ' 中文");
    let original = fs::read(server.join(".mcpm/server.json")).unwrap();
    success(&fixture.register(&[]));
    assert_eq!(
        fs::read(server.join(".mcpm/server.json")).unwrap(),
        original
    );
    assert!(!server.join("mods").exists());
    let workspace = fixture.workspace();
    assert!(!workspace.join("mods").exists());
    let settings: Value =
        serde_json::from_slice(&fs::read(workspace.join(".mcpm/server.json")).unwrap()).unwrap();
    assert_eq!(
        settings,
        json!({"version":2,"ssh":fixture.connection,"ssh_mcpm":env!("CARGO_BIN_EXE_mcpm"),"path":"server ' 中文"})
    );
    // The caller's cwd lacks keys/key; the saved connection must run in workspace.
    let info = run(
        fixture.root.path(),
        &["server", "info", workspace.to_str().unwrap()],
    );
    success(&info);
    assert!(String::from_utf8_lossy(&info.stdout).contains("1.21.1"));
    assert!(!server.join("mods").exists());
    let stored = fs::read(workspace.join(".mcpm/server.json")).unwrap();
    let info_again = fixture.register(&[]);
    failure(&info_again, "already exists");
    success(&fixture.register(&["--force"]));
    assert_eq!(
        fs::read(workspace.join(".mcpm/server.json")).unwrap(),
        stored
    );

    // Compare automatic and explicit routing for mod selection/inspection/provider boundaries.
    fs::create_dir(server.join("mods")).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        server.join("mods/example.jar"),
    )
    .unwrap();
    for subcommand in ["list", "info", "deps", "identify"] {
        let mut tail = match subcommand {
            "list" => vec!["server", "mods", "list", "--no-ui"],
            "identify" => vec![
                "server",
                "mods",
                "source",
                "identify",
                "absent.jar",
                "--source",
                "modrinth",
            ],
            name => vec!["server", "mods", name, "example.jar"],
        };
        if matches!(subcommand, "list" | "info") {
            tail.push("--no-cache");
        }
        tail.extend(["--server", workspace.to_str().unwrap()]);
        let auto = run(fixture.root.path(), &tail);
        let last = tail.len() - 1;
        tail[last] = "server ' 中文";
        let mut explicit = vec![
            "--ssh",
            fixture.connection.as_str(),
            "--ssh-mcpm",
            env!("CARGO_BIN_EXE_mcpm"),
        ];
        explicit.extend(tail);
        let manual = run(&workspace, &explicit);
        assert_eq!(
            auto.status.code(),
            manual.status.code(),
            "{subcommand}: {auto:?} {manual:?}"
        );
        assert_eq!(auto.stdout, manual.stdout, "{subcommand}");
        if subcommand != "identify" {
            success(&auto);
        } else {
            failure(&auto, "absent.jar");
        }
    }
    for command in ["check", "dep-graph"] {
        let args = if command == "check" {
            vec![
                "server",
                "mods",
                command,
                "--server",
                workspace.to_str().unwrap(),
            ]
        } else {
            vec![
                "server",
                "mods",
                command,
                "build",
                "--server",
                workspace.to_str().unwrap(),
            ]
        };
        failure(
            &run(fixture.root.path(), &args),
            "does not support Remote workspaces",
        );
    }
    success(&run(
        fixture.root.path(),
        &[
            "server",
            "init",
            workspace.to_str().unwrap(),
            "--force",
            "--minecraft-version",
            "1.21.3",
            "--loader",
            "vanilla",
        ],
    ));
    assert_eq!(
        fs::read(workspace.join(".mcpm/server.json")).unwrap(),
        stored
    );
    let updated: Value =
        serde_json::from_slice(&fs::read(server.join(".mcpm/server.json")).unwrap()).unwrap();
    assert_eq!(updated["minecraft_version"], "1.21.3");
    assert_eq!(updated["loader"], Value::Null);

    // Compatibility-provider commands must also route first and reject a chained target.
    write(&server, "mcpm.json", json!({"version":1,"type":"remote"}));
    for command in ["search", "files"] {
        let output = run(
            fixture.root.path(),
            &[
                "server",
                "mods",
                "source",
                command,
                "example",
                "--server",
                workspace.to_str().unwrap(),
                "--source",
                "modrinth",
            ],
        );
        failure(&output, "requires a Local Server");
        failure(&output, "server ' 中文");
    }
    assert!(!workspace.join("mods").exists());
    let log = fs::read_to_string(fixture.root.path().join("cwd.log")).unwrap();
    assert!(
        log.lines()
            .all(|line| Path::new(line) == workspace.canonicalize().unwrap())
    );
}

#[cfg(unix)]
#[test]
fn explicit_ssh_initializes_on_host_and_never_reads_local_marker_or_converts_types() {
    let fixture = Fixture::new();
    // Explicit SSH ignores a broken local marker at the same textual path.
    let trap = fixture.workspace().join("new");
    write(&trap, "mcpm.json", json!({"broken":true}));
    fs::create_dir(fixture.host().join("new")).unwrap();
    let args = [
        "--ssh",
        &fixture.connection,
        "--ssh-mcpm",
        env!("CARGO_BIN_EXE_mcpm"),
        "server",
        "init",
        "new",
        "--minecraft-version",
        "1.21.1",
        "--loader",
        "vanilla",
    ];
    success(&run(&fixture.workspace(), &args));
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.host().join("new/.mcpm/mcpm.json")).unwrap())
            .unwrap();
    assert_eq!(marker["type"], "local");
    assert!(!trap.join(".mcpm/server.json").exists());
    failure(
        &run(
            fixture.root.path(),
            &["server", "info", trap.to_str().unwrap()],
        ),
        "invalid workspace manifest",
    );

    // A Remote reference cannot target another Remote reference.
    write(
        &fixture.host().join("server ' 中文"),
        "mcpm.json",
        json!({"version":1,"type":"remote"}),
    );
    failure(&fixture.register(&[]), "requires a Local Server");
    assert!(!fixture.workspace().join(".mcpm").exists());
    local(&fixture.host().join("server ' 中文"));
    local(&fixture.workspace());
    let old = fs::read(fixture.workspace().join(".mcpm/server.json")).unwrap();
    failure(
        &fixture.register(&["--force"]),
        "cannot convert a Local workspace",
    );
    assert_eq!(
        fs::read(fixture.workspace().join(".mcpm/server.json")).unwrap(),
        old
    );
}
