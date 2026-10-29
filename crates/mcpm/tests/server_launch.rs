//! Controlled foreground processes verify argv, environment, stream inheritance and signal reaping.

#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn initialize(root: &Path, body: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .args([
            "init",
            "--no-git",
            "--minecraft-version",
            "1.21.1",
            "--loader",
            "vanilla",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::create_dir(root.join("work")).unwrap();
    fs::write(root.join("runner"), format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(root.join("runner"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.join("conf.toml"), "[java]\nexecutable='./runner'\n[application]\nextraArguments=['-XmxApplicationOnly', '', 'space value']\n").unwrap();
    fs::write(root.join("launch.toml"), "[process]\nworkingDirectory='work'\n[process.environment]\nPROBE_VALUE='value with spaces'\n[jvm]\nextraArguments=['-Dmessage=quoted value', '-cp', 'a path']\n[entry]\ntype='class'\nmainClass='example.Main'\n[application]\nextraArguments=['fixed']\n").unwrap();
}

#[test]
fn foreground_preserves_streams_cwd_environment_argv_and_nonzero_exit() {
    let root = tempfile::tempdir().unwrap();
    initialize(
        root.path(),
        "printf '%s\\n' \"$PWD\" \"$PROBE_VALUE\" \"$@\" > observation\nread -r line\nprintf 'echo:%s\\n' \"$line\"\nprintf 'child stderr\\n' >&2\nexit 37",
    );
    for binary in [
        env!("CARGO_BIN_EXE_mcpm"),
        env!("CARGO_BIN_EXE_mcpm-server"),
        env!("CARGO_BIN_EXE_mcpm-tools"),
    ] {
        let mut command = Command::new(binary);
        if binary == env!("CARGO_BIN_EXE_mcpm-tools") {
            command.arg("server");
        }
        let mut child = command
            .arg("launch")
            .arg("--server")
            .arg(root.path())
            .arg("--raw")
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "console input").unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(37), "{output:?}");
        assert_eq!(output.stdout, b"echo:console input\n");
        assert_eq!(output.stderr, b"child stderr\n");
        let values = fs::read_to_string(root.path().join("work/observation")).unwrap();
        let expected = format!(
            "{}\nvalue with spaces\n-Dmessage=quoted value\n-cp\na path\nexample.Main\nfixed\n-XmxApplicationOnly\n\nspace value\n",
            fs::canonicalize(root.path().join("work"))
                .unwrap()
                .display()
        );
        assert_eq!(values, expected);
    }
}

#[test]
fn parent_termination_reaches_child_and_waits_for_its_shutdown_status() {
    let root = tempfile::tempdir().unwrap();
    initialize(
        root.path(),
        "trap 'printf stopped > shutdown; exit 73' TERM INT\nprintf '%s' \"$$\" > child.pid\nwhile :; do sleep 1; done",
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("launch")
        .arg("--server")
        .arg(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !root.path().join("work/child.pid").exists() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "child did not become ready"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "launch exited before the probe became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            panic!("launch failed to finish after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(73), "{output:?}");
    assert_eq!(
        fs::read(root.path().join("work/shutdown")).unwrap(),
        b"stopped"
    );
    let pid = fs::read_to_string(root.path().join("work/child.pid")).unwrap();
    assert!(
        !Command::new("/bin/kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "Java child was left running"
    );
}

#[test]
fn launch_overrides_are_typed_ordered_and_do_not_rewrite_configuration() {
    let root = tempfile::tempdir().unwrap();
    initialize(
        root.path(),
        "case \" $* \" in *' -XX:+PrintFlagsFinal '*) printf 'bool UseG1GC = true\\n'; printf 'openjdk version \"26\"\\n' >&2; exit 0;; esac\nprintf '%s\\n' \"$@\" > observation\nexit 29",
    );
    let launch = fs::read_to_string(root.path().join("launch.toml"))
        .unwrap()
        .replace("example.Main", "net.minecraft.server.Main");
    fs::write(root.path().join("launch.toml"), &launch).unwrap();
    let conf = fs::read(root.path().join("conf.toml")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("launch")
        .arg("--server")
        .arg(root.path())
        .args([
            "--java",
            "./runner",
            "-m1G:2G",
            "-Gg1",
            "--port",
            "25566",
            "--world",
            "new world",
            "-E",
            "-Dlast=one argument",
            "-Ojvm.extraArguments=-Ddiscard=yes",
            "-Ojvm.extraArguments:=",
            "-Ojvm.extraArguments=-Done=1",
            "-Ojvm.extraArguments+-Dtwo=2",
            "-Oapplication.extraArguments:=['replacement', '']",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(29), "{output:?}");
    assert_eq!(
        fs::read_to_string(root.path().join("work/observation")).unwrap(),
        "-Dmessage=quoted value\n-cp\na path\n-Xms1G\n-Xmx2G\n-XX:+UseG1GC\n-Done=1\n-Dtwo=2\n-Dlast=one argument\nnet.minecraft.server.Main\nfixed\n--port\n25566\n--world\nnew world\nreplacement\n\n"
    );
    assert_eq!(fs::read(root.path().join("conf.toml")).unwrap(), conf);
    assert_eq!(
        fs::read_to_string(root.path().join("launch.toml")).unwrap(),
        launch
    );
    fs::remove_file(root.path().join("work/observation")).unwrap();
    for arguments in [
        vec!["-m2G", "-Ojvm.heap.maximum=4G"],
        vec!["--world", "one", "-Oserver.world=two"],
        vec!["--java", "./runner", "--java", "./runner"],
        vec!["-E-Done=1", "-E-Dtwo=2"],
        vec!["-Gauto"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .arg("launch")
            .arg("--server")
            .arg(root.path())
            .args(&arguments)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{arguments:?}: {output:?}");
        assert!(!root.path().join("work/observation").exists());
    }
}

#[test]
fn launch_leaves_jvm_and_application_legality_to_the_selected_process() {
    let root = tempfile::tempdir().unwrap();
    initialize(root.path(), "printf '%s\\n' \"$@\" > observation\nexit 29");
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("launch")
        .arg("--server")
        .arg(root.path())
        .env("JAVA_TOOL_OPTIONS", "unterminated'quote")
        .args([
            "-m3G:0",
            "-Gserial",
            "--port",
            "0",
            "--world",
            "",
            "-Ojvm.heap.maximumPercentage=200",
            "-Ojvm.gc.g1.reservePercentage=90",
            "-Ojvm.gc.concurrentThreads=0",
            "-Ojvm.memory.extraArguments=-Xmxnot-a-size",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(29), "{output:?}");
    let observed = fs::read_to_string(root.path().join("work/observation")).unwrap();
    assert!(observed.contains("-Xms3G\n-Xmx0\n-XX:MaxRAMPercentage=200\n"));
    assert!(observed.contains("-Xmxnot-a-size\nexample.Main\nfixed\n--port\n0\n--world\n\n"));
}

#[test]
fn authentication_override_resolves_manual_jar_from_root_and_leaves_managed_state_untouched() {
    let root = tempfile::tempdir().unwrap();
    initialize(root.path(), "printf '%s\\n' \"$@\" > observation\nexit 29");
    fs::write(
        root.path().join("manual agent.jar"),
        b"user supplied file; no hash validation",
    )
    .unwrap();
    fs::create_dir(root.path().join(".mcpm/authlib-injector")).unwrap();
    let meta = root.path().join(".mcpm/authlib-injector/meta.json");
    fs::write(&meta, b"unreadable as metadata").unwrap();
    let original = fs::read(root.path().join("conf.toml")).unwrap();
    for flag in ["--auth", "-A"] {
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .arg("launch")
            .arg("--server")
            .arg(root.path())
            .args([
                flag,
                "https://example.com/api/?a=b",
                "-Oauthlib-injector.jar=manual agent.jar",
                "-Oauthlib-injector.noLogFile=true",
            ])
            .current_dir("/")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(29), "{output:?}");
        let observed = fs::read_to_string(root.path().join("work/observation")).unwrap();
        let agent = format!(
            "-javaagent:{}=https://example.com/api/?a=b\n",
            fs::canonicalize(root.path())
                .unwrap()
                .join("manual agent.jar")
                .display()
        );
        assert!(observed.contains(&agent), "{observed}");
        assert!(observed.contains("-Dauthlibinjector.noLogFile\nexample.Main\n"));
        assert_eq!(fs::read(root.path().join("conf.toml")).unwrap(), original);
        assert_eq!(fs::read(&meta).unwrap(), b"unreadable as metadata");
    }
    fs::remove_file(root.path().join("work/observation")).unwrap();
    for arguments in [
        vec![
            "-A",
            "https://example.com/",
            "-Oauthlib-injector.apiUrl=https://other.example/",
        ],
        vec!["--auth-jar-policy", "local"],
        vec!["-A", "https://example.com/", "--auth-jar-policy", "local"],
        vec![
            "-A",
            "https://example.com/",
            "-Oauthlib-injector.jar=manual agent.jar",
            "--auth-jar-policy",
            "auto",
        ],
        vec![
            "-A",
            "https://example.com/",
            "-Oauthlib-injector.jar=manual agent.jar",
            "-E-javaagent:../manual agent.jar=opaque",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .arg("launch")
            .arg("--server")
            .arg(root.path())
            .args(&arguments)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{arguments:?}: {output:?}");
        assert!(!root.path().join("work/observation").exists());
    }
}
