//! Console and Raw presentation for server output.

use std::io::{self, Write};

use super::metadata::{ServerInfo, StartCommand};

use crate::{
    commands::output_style::{OutputStyle, text},
    terminal,
};

pub(super) fn write_snapshot(
    output: &mut impl Write,
    server: &ServerInfo,
    show_full_command: bool,
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
    let (loader_name, loader_version) = match &loader {
        Some(info) => (text(&info.name), text(&info.version)),
        None => ("vanilla or unrecognized".into(), String::new()),
    };
    if !terminal::stdout_is_tty() {
        return writeln!(
            output,
            "{name}\t{path}\t{version}\t{loader_name}\t{loader_version}\t{command}"
        );
    }
    let command = if show_full_command {
        command
    } else {
        truncate_command(
            &command,
            textwrap::termwidth()
                .saturating_sub("Full command: ".len())
                .saturating_sub(1),
        )
    };
    writeln!(output, "{} {}", style.muted("Server:"), style.primary(name))?;
    writeln!(output, "{}{}", style.muted("Path: "), style.muted(path))?;
    writeln!(
        output,
        "{}{}",
        style.muted("Minecraft: "),
        style.secondary(version)
    )?;
    let loader_name = match &loader {
        Some(info) => style.loader(&info.id, &loader_name),
        None => style.muted(loader_name),
    };
    write!(output, "{}{}", style.muted("Loader: "), loader_name)?;
    if loader.is_some() {
        write!(output, " {}", style.secondary(loader_version))?;
    }
    writeln!(output)?;
    if let Some(start) = &server.start {
        writeln!(
            output,
            "{}{}",
            style.muted("Working directory: "),
            style.muted(text(&start.directory))
        )?;
    }
    writeln!(
        output,
        "{}{}",
        style.muted("Full command: "),
        style.muted(command)
    )
}

/// Count omitted Unicode characters while fitting display columns, including the suffix.
fn truncate_command(command: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let chars: Vec<_> = command.chars().collect();
    let total_width: usize = chars.iter().map(|ch| ch.width().unwrap_or(0)).sum();
    if total_width <= width {
        return command.into();
    }
    let mut used = 0;
    let mut best = 0;
    for (index, ch) in chars.iter().enumerate() {
        used += ch.width().unwrap_or(0);
        let omitted = chars.len() - index - 1;
        let suffix = format!(" (+{omitted})");
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

fn full_command(start: &StartCommand) -> String {
    std::iter::once(&start.executable)
        .chain(start.arguments.iter())
        .map(|arg| {
            let quoted = if start.windows {
                windows_quote(arg)
            } else {
                shlex::try_quote(arg)
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| format!("{arg:?}"))
            };
            text(&quoted)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// Windows Java argv quoting. This is a display of argv, not a batch program.
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
