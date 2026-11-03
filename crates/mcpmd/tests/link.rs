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
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(
        matches!(serde_json::from_str::<Ready>(&line).unwrap(), Ready::Ready),
        "{line}"
    );
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
