//! SSH argv parsing and fixed remote daemon invocation, without shell-parsed operation inputs.

use anyhow::{Context, Result, ensure};
use mcpm_exec::operation::Connection;
use std::{path::Path, process::Command};

pub fn command(connection: &Connection) -> Result<Command> {
    let words =
        shlex::split(&connection.ssh).context("invalid quoting in SSH connection command")?;
    let executable = words.first().context("SSH connection command is empty")?;
    ensure!(!executable.is_empty(), "SSH executable is empty");
    let mut command = Command::new(executable);
    if let Some(index) = openssh_target(&words) {
        command.args(["-o", "RequestTTY=no", "-o", "BatchMode=yes"]);
        let options = &words[1..index];
        command.args(
            options
                .strip_suffix(&[String::from("--")])
                .unwrap_or(options),
        );
        command.args(["-T", "--"]).arg(&words[index]);
    } else {
        command.args(&words[1..]);
    }
    let daemon = connection.mcpmd.as_deref().unwrap_or("mcpmd");
    ensure!(
        !daemon.trim().is_empty(),
        "remote mcpmd executable is empty"
    );
    let windows = daemon.contains('\\')
        || daemon.as_bytes().get(1) == Some(&b':')
        || daemon.ends_with(".exe");
    let remote = if windows {
        ensure!(
            !daemon.contains(['\0', '\r', '\n', '"', '%', '!']),
            "remote mcpmd executable for Windows cmd cannot contain NUL, line breaks, quotes, '%' or '!'"
        );
        format!("\"{daemon}\" serve")
    } else {
        shlex::try_join([daemon, "serve"]).context("remote executable cannot contain NUL bytes")?
    };
    command.arg(remote);
    Ok(command)
}

fn openssh_target(words: &[String]) -> Option<usize> {
    if !matches!(
        Path::new(words.first()?).file_name()?.to_str(),
        Some("ssh" | "ssh.exe")
    ) {
        return None;
    }
    let mut index = 1;
    while let Some(word) = words.get(index) {
        if word == "--" {
            index += 1;
            break;
        }
        if !word.starts_with('-') {
            break;
        }
        if word == "-" {
            return None;
        }
        let mut options = word[1..].chars();
        while let Some(option) = options.next() {
            if "BbcDEeFIiJLlmopPRSw".contains(option) {
                if options.as_str().is_empty() {
                    index += 1;
                    words.get(index)?;
                }
                break;
            }
            if !"46AaCgKkMnqsTtvXxYy".contains(option) {
                return None;
            }
        }
        index += 1;
    }
    let destination = words.get(index)?;
    (index + 1 == words.len() && !destination.is_empty() && !destination.starts_with('-'))
        .then_some(index)
}
