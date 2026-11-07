//! SSH argv parsing and fixed remote daemon invocation, without shell-parsed operation inputs.

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use mcpm_exec::operation::Connection;
use std::{
    path::Path,
    process::{Command, Stdio},
};

/// Probe the remote command interpreter, never the local OS or the daemon basename.
pub async fn command(connection: &Connection) -> Result<Command> {
    if let Some(daemon) = &connection.mcpmd {
        ensure!(
            !daemon.trim().is_empty() && !daemon.contains(['\0', '\r', '\n']),
            "remote mcpmd executable is empty or contains NUL or line breaks"
        );
    }
    let mut probe = transport(connection)?;
    probe.arg("echo mcpm-shell:%OS%");
    let output = tokio::process::Command::from(probe)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("failed to probe remote SSH command interpreter")?;
    ensure!(
        output.status.success(),
        "SSH interpreter probe exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let marker = std::str::from_utf8(&output.stdout)
        .context("remote SSH interpreter probe is not UTF-8")?
        .trim();
    let remote = match marker {
        "mcpm-shell:Windows_NT" => {
            // Encoding the script avoids cmd expansion of quotes, %, ! and metacharacters.
            let override_path = connection.mcpmd.as_deref().map_or_else(
                || "$null".to_owned(),
                |path| format!("'{}'", path.replace('\'', "''")),
            );
            let script = format!(
                "$daemon = {override_path}\n{}",
                include_str!("bootstrap.ps1")
            );
            let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
            format!(
                "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
                STANDARD.encode(bytes)
            )
        }
        "mcpm-shell:%OS%" => match &connection.mcpmd {
            Some(daemon) => shlex::try_join([
                "sh",
                "-c",
                include_str!("bootstrap.sh"),
                "mcpm-bootstrap",
                daemon.as_str(),
            ])?,
            None => shlex::try_join(["sh", "-c", include_str!("bootstrap.sh"), "mcpm-bootstrap"])?,
        },
        _ => bail!(
            "unsupported remote SSH command interpreter: {marker:?}; expected POSIX shell or Windows cmd"
        ),
    };
    let mut command = transport(connection)?;
    command.arg(remote);
    Ok(command)
}

fn transport(connection: &Connection) -> Result<Command> {
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
