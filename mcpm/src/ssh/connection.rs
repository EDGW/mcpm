//! Builds connection processes with best-effort OpenSSH destination recognition.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn command(source: &str) -> Result<(Command, Option<String>)> {
    let words = shlex::split(source).context("invalid quoting in --ssh connection command")?;
    let executable = words.first().context("--ssh connection command is empty")?;
    ensure!(!executable.is_empty(), "--ssh executable is empty");
    let mut command = Command::new(executable);
    let Some(target) = target(&words) else {
        // Unknown connection tools and option syntax retain their original argv.
        command.args(&words[1..]);
        return Ok((command, None));
    };
    command.args(["-o", "RequestTTY=no"]);
    let options = &words[1..target.index];
    command.args(
        options
            .strip_suffix(&[String::from("--")])
            .unwrap_or(options),
    );
    // -T must follow connection flags, so even bundled -tt cannot enable a PTY.
    command.args(["-T", "--"]).arg(&words[target.index]);
    tracing::debug!(
        executable,
        destination = target.name,
        option_words = options.len(),
        "Recognized SSH connection and disabled PTY allocation"
    );
    Ok((command, Some(target.name)))
}

pub(super) fn destination(source: &str) -> Option<String> {
    target(&shlex::split(source)?).map(|target| target.name)
}

struct Target {
    index: usize,
    name: String,
}

/// Only recognize known OpenSSH syntax; display extraction never validates a command.
fn target(words: &[String]) -> Option<Target> {
    if !matches!(
        Path::new(words.first()?)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("ssh" | "ssh.exe")
    ) {
        return None;
    }
    let mut index = 1;
    let mut user = None;
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
                let value = if options.as_str().is_empty() {
                    index += 1;
                    words.get(index)?.as_str()
                } else {
                    options.as_str()
                };
                if option == 'l' {
                    user.get_or_insert(value);
                } else if option == 'o' {
                    let (key, value) = value.split_once(['=', ' ']).unwrap_or((value, ""));
                    if key.eq_ignore_ascii_case("User") {
                        user.get_or_insert(value.trim_start_matches(['=', ' ']));
                    }
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
    if index + 1 != words.len() || destination.is_empty() || destination.starts_with('-') {
        return None;
    }
    let name = match user.filter(|user| !user.is_empty()) {
        Some(user) if !destination.starts_with("ssh://") => {
            let host = destination
                .rsplit_once('@')
                .map_or(destination.as_str(), |(_, host)| host);
            format!("{user}@{host}")
        }
        _ => destination.clone(),
    };
    Some(Target { index, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_option_values_and_forces_non_pty_after_connection_flags() {
        let (command, destination) =
            command("ssh -4tt -i 'key with spaces' -p2222 -o RequestTTY=force user@host").unwrap();
        assert_eq!(destination.as_deref(), Some("user@host"));
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
    }

    #[test]
    fn recognizes_destinations_without_confusing_option_values_with_hosts() {
        for (source, expected) in [
            ("ssh -J jump@proxy -p2222 user@host", Some("user@host")),
            ("ssh -o 'ProxyCommand=helper %h %p' -- host", Some("host")),
            ("ssh -l alice host", Some("alice@host")),
            ("ssh -lalice host", Some("alice@host")),
            ("ssh -o User=alice host", Some("alice@host")),
            ("ssh -o 'User alice' host", Some("alice@host")),
            ("ssh -l alice bob@host", Some("alice@host")),
            ("ssh -l alice -l bob host", Some("alice@host")),
            ("ssh -o User=alice -l bob host", Some("alice@host")),
            ("ssh 用户@主机", Some("用户@主机")),
            ("ssh -Z value host", None),
            ("ssh -i", None),
            ("ssh -p 2222", None),
            ("ssh host command", None),
            ("ssh 'host", None),
            ("custom-ssh --profile production", None),
        ] {
            assert_eq!(destination(source).as_deref(), expected, "{source}");
        }
    }

    #[test]
    fn unrecognized_syntax_preserves_connection_argv_without_rejecting_it() {
        for source in [
            "custom-ssh --profile 'production host'",
            "ssh -Z custom host",
            "ssh host command",
            "ssh -N host",
            "ssh -i",
        ] {
            let words = shlex::split(source).unwrap();
            let (command, destination) = command(source).unwrap();
            assert!(destination.is_none(), "{source}");
            assert_eq!(command.get_program(), words[0].as_str());
            assert_eq!(
                command
                    .get_args()
                    .map(|argument| argument.to_str().unwrap().to_owned())
                    .collect::<Vec<_>>(),
                &words[1..]
            );
        }
        for invalid in ["", "''", "ssh 'host"] {
            assert!(command(invalid).is_err(), "{invalid}");
        }
    }
}
