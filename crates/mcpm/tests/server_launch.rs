//! Launch through the persistent shim: argv, environment, working directory, overrides and the
//! attached Raw session, observed through probe files and the attach output.

#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, ChildStdin, Command, Output, Stdio},
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

/// Starts an attached Raw launch over stdin; the returned stdin keeps the session attached.
fn attached(binary: &str, root: &Path, arguments: &[&str]) -> (Child, ChildStdin) {
    let mut command = Command::new(binary);
    if binary == env!("CARGO_BIN_EXE_mcpm-tools") {
        command.arg("server");
    }
    let mut child = command
        .arg("launch")
        .arg("--server")
        .arg(root)
        .args(["--raw", "-c", "stdin"])
        .args(arguments)
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    (child, stdin)
}

/// Launches without attaching; Java keeps running in the shim after this returns.
fn detached(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("launch")
        .arg("--server")
        .arg(root)
        .args(["--raw", "-D"])
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn wait_for(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{path:?} did not appear"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The probe writes its argv as the last action before exiting with `code`.
fn observing(code: u8) -> String {
    format!(
        "case \" $* \" in *' -XX:+PrintFlagsFinal '*) printf 'bool UseG1GC = true\\n'; printf 'openjdk version \"26\"\\n' >&2; exit 0;; esac\nprintf '%s\\n' \"$@\" > observation.tmp\nmv observation.tmp observation\nexit {code}"
    )
}

#[test]
fn attached_launch_relays_console_input_streams_cwd_environment_argv_and_exit_code() {
    for binary in [
        env!("CARGO_BIN_EXE_mcpm"),
        env!("CARGO_BIN_EXE_mcpm-server"),
        env!("CARGO_BIN_EXE_mcpm-tools"),
    ] {
        let root = tempfile::tempdir().unwrap();
        initialize(
            root.path(),
            "printf '%s\\n' \"$PWD\" \"$PROBE_VALUE\" \"$@\" > observation\nread -r line\nprintf 'echo:%s\\n' \"$line\"\nprintf 'child stderr\\n' >&2\nexit 37",
        );
        let (child, mut stdin) = attached(binary, root.path(), &[]);
        writeln!(stdin, "console input").unwrap();
        // Keep stdin open: EOF detaches, and the session must end with Java's own exit.
        let output = child.wait_with_output().unwrap();
        drop(stdin);
        assert_eq!(output.status.code(), Some(37), "{output:?}");
        assert_eq!(output.stdout, b"echo:console input\n");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("[java stderr] child stderr\n"), "{stderr}");
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
fn interrupt_stops_java_through_its_console_and_detaching_leaves_it_running() {
    let root = tempfile::tempdir().unwrap();
    initialize(
        root.path(),
        "printf '%s' \"$$\" > child.pid\nwhile read -r line; do\n  if [ \"$line\" = stop ]; then printf stopped > shutdown; exit 73; fi\ndone\nsleep 30",
    );
    let (child, stdin) = attached(env!("CARGO_BIN_EXE_mcpm"), root.path(), &[]);
    wait_for(&root.path().join("work/child.pid"));
    assert!(
        Command::new("/bin/kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    assert_eq!(output.status.code(), Some(73), "{output:?}");
    assert_eq!(
        fs::read(root.path().join("work/shutdown")).unwrap(),
        b"stopped"
    );

    // Detach on EOF: the shim, not the CLI, owns Java.
    fs::remove_file(root.path().join("work/child.pid")).unwrap();
    let (child, stdin) = attached(env!("CARGO_BIN_EXE_mcpm"), root.path(), &[]);
    wait_for(&root.path().join("work/child.pid"));
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let pid = fs::read_to_string(root.path().join("work/child.pid")).unwrap();
    let alive = || {
        Command::new("/bin/kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(alive(), "detaching must not stop Java");
    let (child, mut stdin) = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("attach")
        .arg("--server")
        .arg(root.path())
        .arg("--raw")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map(|mut child| {
            let stdin = child.stdin.take().unwrap();
            (child, stdin)
        })
        .unwrap();
    writeln!(stdin, "stop").unwrap();
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    assert_eq!(output.status.code(), Some(73), "{output:?}");
    assert!(!alive(), "Java was left running");
}

#[test]
fn launch_overrides_are_typed_ordered_and_do_not_rewrite_configuration() {
    let root = tempfile::tempdir().unwrap();
    initialize(root.path(), &observing(29));
    let launch = fs::read_to_string(root.path().join("launch.toml"))
        .unwrap()
        .replace("example.Main", "net.minecraft.server.Main");
    fs::write(root.path().join("launch.toml"), &launch).unwrap();
    let conf = fs::read(root.path().join("conf.toml")).unwrap();
    let output = detached(
        root.path(),
        &[
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
        ],
    );
    assert!(output.status.success(), "{output:?}");
    wait_for(&root.path().join("work/observation"));
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
        vec!["--port", "25565", "-p25566"],
        vec!["-p65536"],
        vec!["--java", "./runner", "--java", "./runner"],
        vec!["-E-Done=1", "-E-Dtwo=2"],
        vec!["-Gauto"],
    ] {
        let output = detached(root.path(), &arguments);
        assert!(!output.status.success(), "{arguments:?}: {output:?}");
        assert!(!root.path().join("work/observation").exists());
    }
}

#[test]
fn launch_leaves_jvm_and_application_legality_to_the_selected_process() {
    let root = tempfile::tempdir().unwrap();
    initialize(root.path(), &observing(29));
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("launch")
        .arg("--server")
        .arg(root.path())
        .args(["--raw", "-D"])
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
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    wait_for(&root.path().join("work/observation"));
    let observed = fs::read_to_string(root.path().join("work/observation")).unwrap();
    assert!(
        observed.contains("-Xms3G\n-Xmx0\n-XX:MaxRAMPercentage=200\n"),
        "{observed}"
    );
    assert!(
        observed.contains("-Xmxnot-a-size\nexample.Main\nfixed\n--port\n0\n--world\n\n"),
        "{observed}"
    );
}

#[test]
fn authentication_override_resolves_manual_jar_from_root_and_leaves_managed_state_untouched() {
    let root = tempfile::tempdir().unwrap();
    initialize(root.path(), &observing(29));
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
        let output = detached(
            root.path(),
            &[
                flag,
                "https://example.com/api/?a=b",
                "-Oauthlib-injector.jar=manual agent.jar",
                "-Oauthlib-injector.noLogFile=true",
            ],
        );
        assert!(output.status.success(), "{output:?}");
        wait_for(&root.path().join("work/observation"));
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
        fs::remove_file(root.path().join("work/observation")).unwrap();
        wait_until_exited(root.path());
    }
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
        let output = detached(root.path(), &arguments);
        assert!(!output.status.success(), "{arguments:?}: {output:?}");
        assert!(!root.path().join("work/observation").exists());
    }
}

/// A later launch is refused while the previous Java is still active.
fn wait_until_exited(root: &Path) {
    let started = Instant::now();
    loop {
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .arg("attach")
            .arg("--server")
            .arg(root)
            .arg("--raw")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        if String::from_utf8_lossy(&output.stderr).contains("Java has exited") {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "previous Java did not exit: {output:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
