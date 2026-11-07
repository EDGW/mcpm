//! Real link/serve lifecycle and startup-cause regressions with a local stdio connector.

#![cfg(unix)]

use mcpm_exec::{operation::Connection, service::control::Ready};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Link(Child);
impl Drop for Link {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpmd"));
    command.env("XDG_RUNTIME_DIR", root).env("HOME", root);
    command
}
fn connection(root: &Path) -> Connection {
    let connector = root.join("connector.sh");
    fs::write(
        &connector,
        "#!/bin/sh\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
    )
    .unwrap();
    Connection {
        ssh: shlex::try_join(["/bin/sh", connector.to_str().unwrap()]).unwrap(),
        mcpmd: Some(env!("CARGO_BIN_EXE_mcpmd").into()),
    }
}
fn start(root: &Path, connection: &Connection, idle: u64) -> Link {
    let mut child = command(root)
        .args(["link", "--ready", "--connection"])
        .arg(serde_json::to_string(connection).unwrap())
        .arg("--idle-timeout")
        .arg(idle.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        match serde_json::from_str::<Ready>(&line).unwrap() {
            Ready::Warning(_) => continue,
            Ready::Ready => break,
            other => panic!("link failed to start: {other:?}"),
        }
    }
    Link(child)
}
fn exited(link: &mut Link) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = link.0.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            return;
        }
        assert!(Instant::now() < deadline, "link failed to stop");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn stop_by_ssh_finds_links_started_with_an_explicit_remote_daemon_path() {
    let root = tempfile::Builder::new()
        .prefix("ml-")
        .tempdir_in("/tmp")
        .unwrap();
    let connection = connection(root.path());
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    fs::write(root.path().join(".mcpm/mcpmd.path"), "invalid record\n").unwrap();
    let mut link = start(root.path(), &connection, 0);
    let output = command(root.path())
        .args(["link", "stop", &connection.ssh])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    exited(&mut link);
    let output = command(root.path())
        .args(["link", "list"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&connection.key().unwrap()));
}

#[test]
fn idle_link_exits_without_clients_and_removes_its_state() {
    let root = tempfile::Builder::new()
        .prefix("mi-")
        .tempdir_in("/tmp")
        .unwrap();
    let connection = connection(root.path());
    let mut link = start(root.path(), &connection, 1);
    exited(&mut link);
    let output = command(root.path())
        .args(["link", "list"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&connection.key().unwrap()));
}

#[test]
fn startup_failure_keeps_ssh_stderr_and_exit_status_in_the_readiness_cause_chain() {
    let root = tempfile::Builder::new()
        .prefix("mf-")
        .tempdir_in("/tmp")
        .unwrap();
    let failure = root.path().join("failure.sh");
    fs::write(
        &failure,
        "#!/bin/sh\necho authentication-denied-regression >&2\nexit 42\n",
    )
    .unwrap();
    let connection = Connection {
        ssh: shlex::try_join(["/bin/sh", failure.to_str().unwrap()]).unwrap(),
        mcpmd: None,
    };
    let output = command(root.path())
        .args(["link", "--ready", "--connection"])
        .arg(serde_json::to_string(&connection).unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let ready: Ready = serde_json::from_slice(&output.stdout).unwrap();
    let Ready::Error(error) = ready else {
        panic!("expected a startup error")
    };
    let error = error.into_error();
    let chain = format!("{error:#}");
    assert!(
        chain.contains("authentication-denied-regression"),
        "{chain}"
    );
    assert!(chain.contains("42"), "{chain}");
}

fn discovery_connection(root: &Path) -> Connection {
    let mut connection = connection(root);
    connection.mcpmd = None;
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink("/bin/sh", bin.join("sh")).unwrap();
    std::os::unix::fs::symlink("/usr/bin/cmp", bin.join("cmp")).unwrap();
    fs::write(
        root.join("connector.sh"),
        format!(
            "#!/bin/sh\nPATH={}; export PATH\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
            shlex::try_join([bin.to_str().unwrap()]).unwrap()
        ),
    )
    .unwrap();
    connection
}

fn stop(root: &Path, connection: &Connection, link: &mut Link) {
    let output = command(root)
        .args(["link", "stop", &connection.ssh])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    exited(link);
}

#[test]
fn remote_record_precedes_path_and_its_contents_are_not_shell_code() {
    let root = tempfile::Builder::new()
        .prefix("mr-")
        .tempdir_in("/tmp")
        .unwrap();
    let connection = discovery_connection(root.path());
    let daemon = root.path().join("daemon '$(touch injected)' with spaces");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_mcpmd"), &daemon).unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    fs::write(
        root.path().join(".mcpm/mcpmd.path"),
        format!("{}\n", daemon.display()),
    )
    .unwrap();
    let mut link = start(root.path(), &connection, 0);
    stop(root.path(), &connection, &mut link);
    assert!(!root.path().join("injected").exists());
}

#[test]
fn remote_missing_record_uses_mcpmd_on_remote_path() {
    let root = tempfile::Builder::new()
        .prefix("mp-")
        .tempdir_in("/tmp")
        .unwrap();
    let connection = discovery_connection(root.path());
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_mcpmd"), root.path().join("bin/mcpmd")).unwrap();
    let mut link = start(root.path(), &connection, 0);
    stop(root.path(), &connection, &mut link);
}

#[test]
fn remote_bad_records_do_not_fall_back_to_a_different_daemon() {
    for record in [
        "relative/mcpmd\n",
        "/nonexistent/mcpmd\n",
        "/bin/sh\n/bin/sh\n",
        "",
    ] {
        let root = tempfile::Builder::new()
            .prefix("mb-")
            .tempdir_in("/tmp")
            .unwrap();
        let connection = discovery_connection(root.path());
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_mcpmd"), root.path().join("bin/mcpmd"))
            .unwrap();
        fs::create_dir(root.path().join(".mcpm")).unwrap();
        fs::write(root.path().join(".mcpm/mcpmd.path"), record).unwrap();
        let output = command(root.path())
            .args(["link", "--ready", "--connection"])
            .arg(serde_json::to_string(&connection).unwrap())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        let Ready::Error(error) = serde_json::from_slice::<Ready>(&output.stdout).unwrap() else {
            panic!("bad record unexpectedly started a link")
        };
        assert_eq!(
            error.executable_not_found.is_some(),
            record == "/nonexistent/mcpmd\n"
        );
    }
}

#[test]
fn missing_record_warns_then_reports_error_only_when_path_fallback_also_fails() {
    let root = tempfile::Builder::new()
        .prefix("mw-")
        .tempdir_in("/tmp")
        .unwrap();
    let connection = discovery_connection(root.path());
    let output = command(root.path())
        .args(["link", "--ready", "--connection"])
        .arg(serde_json::to_string(&connection).unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let events: Vec<Ready> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        matches!(events.as_slice(), [Ready::Warning(_), Ready::Error(_)]),
        "{events:?}"
    );
    let Ready::Error(error) = &events[1] else {
        unreachable!()
    };
    assert_eq!(
        error
            .executable_not_found
            .as_ref()
            .map(|missing| (missing.executable.as_str(), missing.remote)),
        Some(("mcpmd", true))
    );
}
