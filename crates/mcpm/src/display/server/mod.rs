//! Console and Raw presentation for server output.

pub(crate) mod attach;
pub(crate) mod launch;

use mcpm_rich::rich;
use std::io::{self, Write};

use crate::{
    display::{
        console::ConsoleWriter,
        rich::line,
        style::{OutputStyle, text},
    },
    server::RemoteInfo,
    terminal,
};
use mcpm_backend::operations::server::info::{
    RconInfo, ServerInfo, ServerLoaderInfo, StartCommand,
};

pub(crate) fn write_snapshot(
    output: &mut impl Write,
    server: &ServerInfo,
    show_full_command: bool,
    remote: Option<&RemoteInfo>,
) -> io::Result<()> {
    let loader = &server.loader;
    let style = OutputStyle::new();
    let name = text(&server.name);
    let path = text(&server.path);
    let version = text(&server.minecraft_version);
    let command = server
        .start
        .as_ref()
        .map(full_command)
        .unwrap_or_else(|| "unavailable".into());
    let (loader_name, loader_version) = match loader {
        ServerLoaderInfo::Modded(info) => (text(&info.name), text(&info.version)),
        ServerLoaderInfo::Vanilla => ("Vanilla".into(), String::new()),
        ServerLoaderInfo::Unrecognized => ("Unrecognized".into(), String::new()),
    };
    if !terminal::stdout_is_tty() {
        if let Some(remote) = remote {
            write!(
                output,
                "{}\t{}\t{}\t",
                text(&remote.path),
                text(&remote_ssh(remote)),
                text(remote.executable.as_str()),
            )?;
        }
        let loader_label = match loader {
            ServerLoaderInfo::Vanilla => "vanilla",
            ServerLoaderInfo::Unrecognized => "unrecognized",
            ServerLoaderInfo::Modded(info) => &info.name,
        };
        return writeln!(
            output,
            "{name}\t{path}\t{version}\t{}\t{loader_version}\t{command}",
            text(loader_label)
        );
    }
    let mut output = ConsoleWriter::new(output);
    if let Some(remote) = remote {
        for (label, value) in [
            ("Remote path: ", text(&remote.path)),
            ("Remote SSH: ", text(&remote_ssh(remote))),
            ("Remote mcpmd: ", text(remote.executable.as_str())),
        ] {
            writeln!(output, "{}{}", style.muted(label), style.muted(value))?;
        }
        let separator = "─".repeat(32.min(textwrap::termwidth().saturating_sub(1)));
        writeln!(output, "\n{}\n", style.muted(separator))?;
    }
    writeln!(
        output,
        "{} {} {}",
        style.primary(name),
        style.normal("at"),
        style.muted(path)
    )?;
    let loader_name = match loader {
        ServerLoaderInfo::Modded(info) => style.loader(&info.id, &loader_name),
        ServerLoaderInfo::Vanilla => style.loader("vanilla", &loader_name),
        ServerLoaderInfo::Unrecognized => style.muted(loader_name),
    };
    write!(output, "{} {}", style.secondary(version), loader_name)?;
    if matches!(loader, ServerLoaderInfo::Modded(_)) {
        write!(output, " {}", style.secondary(loader_version))?;
    }
    writeln!(output)?;
    match server.rcon.as_ref().unwrap_or(&RconInfo::Disabled) {
        RconInfo::Disabled => writeln!(
            output,
            "{}",
            line(&rich!("RCON: [disabled](muted)"), &style)
        )?,
        RconInfo::Enabled {
            port,
            masked_password,
        } => {
            writeln!(output, "{}", line(&rich!("RCON: `{port}`"), &style))?;
            writeln!(
                output,
                "{}",
                line(&rich!("RCON password: [{masked_password}](muted)"), &style)
            )?;
        }
        RconInfo::ParseFailed => writeln!(
            output,
            "{}",
            line(&rich!("RCON: [parse failed](warning)"), &style)
        )?,
        RconInfo::PropertiesParseFailed => writeln!(
            output,
            "{}",
            line(&rich!("[parse failed](warning)"), &style)
        )?,
    }
    writeln!(output)?;
    write!(
        output,
        "{}",
        style.muted(format_args!("metadata version {}", server.metadata_version))
    )?;
    if let Some(configuration) = &server.configuration {
        for (label, present) in [
            ("launch.toml", configuration.launch),
            ("conf.toml", configuration.conf),
        ] {
            let status = format!("{label} {}", if present { "configured" } else { "missing" });
            let content = if present {
                rich!("[{status}](muted)")
            } else {
                rich!("[{status}](warning)")
            };
            write!(output, " {} {}", style.muted("·"), line(&content, &style))?;
        }
    }
    writeln!(output)?;
    if server.start.is_some() || show_full_command {
        let command = if show_full_command {
            command
        } else {
            truncate_command(&command, textwrap::termwidth().saturating_sub(16))
        };
        writeln!(
            output,
            "{}{}",
            style.muted("Start command: "),
            style.muted(command)
        )?;
    }
    Ok(())
}

fn remote_ssh(remote: &RemoteInfo) -> String {
    let command = mask_ssh_command(&remote.ssh);
    match remote.destination.as_deref() {
        Some(destination) => format!("{destination} (full command: {command})"),
        None => command,
    }
}

/// Reveal at most three characters at either end, always hiding the middle.
fn mask_ssh_command(command: &str) -> String {
    let chars: Vec<_> = command.chars().collect();
    let visible = 3.min(chars.len().saturating_sub(1) / 2);
    format!(
        "{}***{}",
        chars[..visible].iter().collect::<String>(),
        chars[chars.len() - visible..].iter().collect::<String>(),
    )
}

fn full_command(start: &StartCommand) -> String {
    std::iter::once(&start.executable)
        .chain(start.arguments.iter())
        .map(|arg| {
            text(&if start.windows {
                windows_quote(arg)
            } else {
                shlex::try_quote(arg)
                    .map(|value| value.into_owned())
                    .unwrap_or_else(|_| format!("{arg:?}"))
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_command(command: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let chars: Vec<_> = command.chars().collect();
    if chars
        .iter()
        .map(|ch| ch.width().unwrap_or(0))
        .sum::<usize>()
        <= width
    {
        return command.into();
    }
    let mut used = 0;
    let mut best = 0;
    for (index, ch) in chars.iter().enumerate() {
        used += ch.width().unwrap_or(0);
        let suffix = format!(" (+{})", chars.len() - index - 1);
        if used + suffix.len() <= width {
            best = index + 1;
        }
        if used > width {
            break;
        }
    }
    let suffix = format!("(+{})", chars.len() - best);
    if best == 0 {
        return if suffix.len() <= width {
            suffix
        } else {
            String::new()
        };
    }
    format!("{} {suffix}", chars[..best].iter().collect::<String>())
}

fn windows_quote(value: &str) -> String {
    if !value.is_empty() && !value.contains(|ch: char| ch.is_whitespace() || ch == '"') {
        return value.into();
    }
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        slashes = 0;
        result.push(ch);
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}
