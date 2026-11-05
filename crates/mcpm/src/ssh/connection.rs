//! Best-effort OpenSSH destination recognition for redacted presentation.

use std::path::Path;

pub(super) fn destination(source: &str) -> Option<String> {
    target(&shlex::split(source)?)
}

/// Only recognize known OpenSSH syntax; display extraction never validates a command.
fn target(words: &[String]) -> Option<String> {
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
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
