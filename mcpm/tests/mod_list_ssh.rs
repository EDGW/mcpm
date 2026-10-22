//! CLI coverage for SSH option scope, typed parameters and remote protocol failures.

use std::process::{Command, Stdio};

#[test]
fn unsupported_commands_reject_ssh_before_work() {
    for args in [
        vec!["info", "missing.jar"],
        vec!["deps", "missing.jar"],
        vec!["dep-graph", "build", "."],
        vec!["source", "search", "--source", "modrinth"],
    ] {
        for option in ["--ssh", "--ssh-mcpm"] {
            let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
                .arg("mod")
                .args(&args)
                .args([option, "not-a-connection"])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{:?}", output);
            assert!(output.stdout.is_empty());
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(
                error.contains("supported only") || error.contains("not supported"),
                "{error}"
            );
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .args(["mod", "list", "--ssh-mcpm", "/mcpm"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("requires --ssh")
    );
}

#[test]
fn ssh_options_are_top_level_global_and_unsupported_root_commands_reject_them() {
    for args in [
        vec!["client", "list"],
        vec!["server", "info"],
        vec!["remote"],
        vec!["debug", "palette"],
    ] {
        for option in ["--ssh", "--ssh-mcpm"] {
            let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
                .args([option, "not-a-connection"])
                .args(&args)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("supported only"),
                "{output:?}"
            );
        }
    }
    let help = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--ssh <COMMAND>"));
    assert!(help.contains("--ssh-mcpm <PATH>"));
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
            .prefix("mcpm ssh '")
            .tempdir()
            .unwrap();
        let connection = setup(root.path());
        let executable = root.path().join("mcpm");
        symlink(env!("CARGO_BIN_EXE_mcpm"), &executable).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
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
            "--identify-parallelism",
            "2",
        ];
        let local = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .args(["--raw", "mod", "list"])
            .arg(&fixture)
            .args(options)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(local.status.success(), "{:?}", local);
        for explicit in [false, true] {
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
            if explicit {
                command.arg("--ssh-mcpm").arg(&executable);
            }
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            writeln!(child.stdin.take().unwrap(), "{}", fixture.display()).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{:?}", output);
            assert!(output.stderr.is_empty(), "{:?}", output);
            assert_eq!(output.stdout, local.stdout);
            let connections = fs::read_to_string(root.path().join("connections")).unwrap();
            assert_eq!(connections.lines().count(), if explicit { 2 } else { 1 });
        }
    }

    #[test]
    fn forwards_remote_logs_to_local_sinks_and_passes_console_level() {
        for level in ["off", "debug", "trace"] {
            let root = tempfile::tempdir().unwrap();
            let connection = setup(root.path());
            let executable = root.path().join("mcpm");
            symlink(env!("CARGO_BIN_EXE_mcpm"), &executable).unwrap();
            let fixture =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
            let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
                .args([
                    "--raw",
                    "--log-level",
                    level,
                    "--ssh",
                    &connection,
                    "--ssh-mcpm",
                ])
                .arg(&executable)
                .args(["mod", "list"])
                .arg(fixture)
                .env("HOME", root.path())
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                !stderr.contains("[Remote]")
                    && !stderr.contains("remote=true")
                    && !stderr.contains('\u{1b}')
            );
            if level == "off" {
                assert!(stderr.is_empty(), "{stderr}");
            } else {
                assert!(
                    stderr.contains(
                        "mcpm::commands::mods::list::remote: Starting remote mod list loading"
                    ),
                    "{stderr}"
                );
            }
            assert_eq!(
                stderr.contains("mcpm::remote::command: Queueing remote command data"),
                level == "trace"
            );
            let files: Vec<_> = fs::read_dir(root.path().join(".mcpm/logs"))
                .unwrap()
                .flat_map(|day| fs::read_dir(day.unwrap().path()).unwrap())
                .map(|file| fs::read_to_string(file.unwrap().path()).unwrap())
                .collect();
            assert_eq!(files.len(), 2);
            let local = files
                .iter()
                .find(|file| file.contains("mcpm::remote_log"))
                .expect("local file contains remote logs even when console is off");
            assert!(
                local.contains("Identifying remote mod file")
                    && local.contains("remote_spans")
                    && local.contains("guid")
            );
            assert!(!local.contains("Queueing remote command data"));
            assert_eq!(
                fs::read_to_string(root.path().join("connections"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
        }
    }

    #[test]
    fn remote_failures_use_remote_label_and_preserve_each_cause() {
        let root = tempfile::tempdir().unwrap();
        let connection = setup(root.path());
        let executable = root.path().join("mcpm");
        symlink(env!("CARGO_BIN_EXE_mcpm"), &executable).unwrap();
        let broken = root.path().join("broken.jar");
        fs::write(&broken, b"invalid jar archive").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .args([
                "--raw",
                "--log-level",
                "off",
                "--ssh",
                &connection,
                "--ssh-mcpm",
            ])
            .arg(executable)
            .args(["mod", "list"])
            .arg(&broken)
            .env("HOME", root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains(&format!(
                "remote error: {}\n  caused by: ",
                broken.display()
            )),
            "{stderr}"
        );
        assert!(
            stderr
                .lines()
                .filter(|line| line.starts_with("  caused by:"))
                .count()
                >= 2,
            "{stderr}"
        );
        assert!(
            stderr.contains("remote error: failed to list mods:"),
            "{stderr}"
        );
        assert!(
            !stderr.lines().any(|line| line.starts_with("error:")),
            "{stderr}"
        );
        assert!(
            !stderr.contains("remote operation failed") && !stderr.contains('\u{1b}'),
            "{stderr}"
        );
    }

    #[test]
    fn rejects_corrupted_remote_streams_and_propagates_transport_exit_status() {
        let root = tempfile::tempdir().unwrap();
        let connection = setup(root.path());
        let remote = root.path().join("remote");
        let nil = "00000000-0000-0000-0000-000000000000";
        let command_id = "mod.list";
        let ready = format!(
            "{{\"guid\":\"{nil}\",\"status\":\"ready\",\"contents\":{{\"version\":2,\"commands\":[\"{command_id}\"],\"max_concurrency\":8}}}}\n"
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
}
