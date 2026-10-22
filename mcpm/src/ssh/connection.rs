//! Parses an SSH connection command while keeping its destination separate from options.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

pub(super) fn command(source: &str) -> Result<Command> {
    let words = shlex::split(source).context("invalid quoting in --ssh connection command")?;
    let executable = words.first().context("--ssh connection command is empty")?;
    ensure!(
        matches!(
            Path::new(executable)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("ssh" | "ssh.exe")
        ),
        "--ssh must begin with ssh or a path to ssh"
    );
    let mut index = 1;
    while let Some(word) = words.get(index) {
        if word == "--" {
            index += 1;
            break;
        }
        if !word.starts_with('-') {
            break;
        }
        ensure!(word != "-", "missing SSH destination");
        let mut options = word[1..].chars().peekable();
        while let Some(option) = options.next() {
            if "fGNOQsVW".contains(option) {
                bail!("SSH option -{option} cannot be used for a remote process session");
            }
            if "BbcDEeFIiJLlmopRSw".contains(option) {
                if options.peek().is_none() {
                    index += 1;
                    ensure!(
                        index < words.len(),
                        "SSH option -{option} is missing its value"
                    );
                }
                break;
            }
        }
        index += 1;
    }
    let destination = words.get(index).context("--ssh is missing user@host")?;
    ensure!(
        index + 1 == words.len(),
        "--ssh must contain only SSH options and a destination; do not include a remote command"
    );
    let mut command = Command::new(executable);
    command.args(["-o", "RequestTTY=no"]);
    let options = &words[1..index];
    command.args(
        options
            .strip_suffix(&[String::from("--")])
            .unwrap_or(options),
    );
    // -T must follow connection flags, so even bundled -tt cannot enable a PTY.
    command.args(["-T", "--"]).arg(destination);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_option_values_and_forces_non_pty_after_connection_flags() {
        let command =
            command("ssh -4tt -i 'key with spaces' -p2222 -o RequestTTY=force user@host").unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "-o",
                "RequestTTY=no",
                "-4tt",
                "-i",
                "key with spaces",
                "-p2222",
                "-o",
                "RequestTTY=force",
                "-T",
                "--",
                "user@host"
            ]
        );
        for invalid in [
            "",
            "sh host",
            "ssh -N host",
            "ssh host command",
            "ssh -i",
            "ssh -p 2222",
            "ssh 'host",
        ] {
            assert!(self::command(invalid).is_err(), "{invalid}");
        }
    }
}
