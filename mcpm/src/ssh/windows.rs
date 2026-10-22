//! PowerShell bootstrap and Windows native argv encoding for non-PTY SSH.

use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};

/// Encode the entire invocation so cmd.exe and PowerShell never reinterpret mod arguments.
pub(super) fn command(arguments: &[&str]) -> Result<String> {
    if arguments.iter().any(|argument| argument.contains('\0')) {
        bail!("remote mcpm arguments cannot contain NUL bytes");
    }
    let executable = arguments[0].replace('\'', "''");
    let parameters = arguments[1..]
        .iter()
        .map(|argument| windows_argument(argument))
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; $s=New-Object System.Diagnostics.ProcessStartInfo; $s.FileName='{executable}'; $s.Arguments='{parameters}'; $s.UseShellExecute=$false; $p=[System.Diagnostics.Process]::Start($s); $p.WaitForExit(); exit $p.ExitCode"
    );
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Ok(format!(
        "powershell.exe -NoProfile -NonInteractive -InputFormat Text -OutputFormat Text -EncodedCommand {}",
        STANDARD.encode(bytes)
    ))
}

/// Apply the Windows argv escaping rules without routing arguments through a shell.
fn windows_argument(argument: &str) -> String {
    let mut quoted = String::from("\"");
    let mut slashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            slashes += 1;
            continue;
        }
        if character == '"' {
            quoted.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            quoted.extend(std::iter::repeat_n('\\', slashes));
        }
        slashes = 0;
        quoted.push(character);
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}
