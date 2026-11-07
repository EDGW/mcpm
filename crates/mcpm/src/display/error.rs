//! Shared local and remote error-chain rendering, independent of logging and transport.

use std::io::{self, Write};

use crate::diagnostics::ErrorReport;

use super::{
    console::ConsoleWriter,
    style::{OutputStyle, text},
};

/// Keeps the shared cause chain and adds typed, command-independent detail renderers.
pub(crate) fn diagnostic(
    output: &mut ConsoleWriter<'_>,
    error: &anyhow::Error,
    remote: bool,
) -> io::Result<()> {
    let has_branches =
        mcpm_exec::error::contains::<mcpm_exec::error::CombinedError>(error.as_ref());
    let report = ErrorReport::capture_with(error.as_ref(), |cause| {
        if cause.is::<crate::diagnostics::RemoteError>() {
            return None;
        }
        Some(if has_branches {
            cause.to_string()
        } else {
            super::backup::verification_message(cause).unwrap_or_else(|| cause.to_string())
        })
    });
    write(output, &report, remote)?;
    super::backup::verification(output, error)
}

/// The diagnostic owner supplies captured causes and decides when stderr is available.
pub(crate) fn write(
    output: &mut ConsoleWriter<'_>,
    error: &ErrorReport,
    remote: bool,
) -> io::Result<()> {
    let label = if remote { "remote error:" } else { "error:" };
    let style = OutputStyle::stderr();
    branch(output, error, label, Some(&style))?;
    executable_tip(output, error, remote, Some(&style))
}

fn branch(
    output: &mut ConsoleWriter<'_>,
    error: &ErrorReport,
    label: &str,
    style: Option<&OutputStyle>,
) -> io::Result<()> {
    if error.message.is_empty() && error.causes.is_empty() && !error.branches.is_empty() {
        return branches(output, &error.branches, label, style);
    }
    line(output, label, &error.message, style)?;
    let mut output = output.subwriter();
    for cause in &error.causes {
        line(&mut output, "caused by:", cause, style)?;
    }
    branches(&mut output, &error.branches, "caused by:", style)
}

fn branches(
    output: &mut ConsoleWriter<'_>,
    errors: &[ErrorReport],
    first: &str,
    style: Option<&OutputStyle>,
) -> io::Result<()> {
    for (index, error) in errors.iter().enumerate() {
        branch(
            output,
            error,
            if index == 0 { first } else { "and:" },
            style,
        )?;
    }
    Ok(())
}

fn line(
    output: &mut ConsoleWriter<'_>,
    label: &str,
    message: &str,
    style: Option<&OutputStyle>,
) -> io::Result<()> {
    match style {
        Some(style) => writeln!(
            output,
            "{} {}",
            style.danger(label),
            style.normal(text(message))
        ),
        None => writeln!(output, "{label} {}", text(message)),
    }
}

fn executable_tip(
    output: &mut ConsoleWriter<'_>,
    error: &ErrorReport,
    remote: bool,
    style: Option<&OutputStyle>,
) -> io::Result<()> {
    fn missing(error: &ErrorReport, remote: bool) -> Option<bool> {
        let own = error
            .executable_not_found
            .as_ref()
            .filter(|missing| remote || missing.remote)
            .map(|missing| missing.remote && missing.executable == "mcpmd");
        error
            .branches
            .iter()
            .filter_map(|branch| missing(branch, remote))
            .chain(own)
            .reduce(|daemon, other| daemon || other)
    }
    let Some(daemon) = missing(error, remote) else {
        return Ok(());
    };
    let daemon_note = if daemon {
        "The `mcpmd.path` file normally records mcpmd's path in mcpm's user configuration directory.\n\
         If the record is missing or is not taking effect, consider reinstalling mcpm.\n"
    } else {
        ""
    };
    super::console::tip(
        output,
        &format!(
            "Tip: If you are connecting to a Unix-like system, mcpm uses non-interactive SSH;\n\
             shell startup files such as `.bashrc` and `.zshrc` may NOT be read.\n\
             We recommend using absolute executable paths.\n\
             {daemon_note}\
             To find executables through PATH instead:\n\
             - zsh: set PATH in `~/.zshenv`.\n\
             - bash or other shells: consider SSH User Environment (`~/.ssh/environment`),\n\
               or the SSH client's SetEnv/SendEnv, if the server permits them through\n\
               PermitUserEnvironment/AcceptEnv."
        ),
        3,
        style,
    )
}

/// Plain diagnostic lines for TUI layouts; shares the Console branching and indentation rules.
pub(crate) fn lines(error: &ErrorReport, remote: bool) -> Vec<String> {
    let mut bytes = Vec::new();
    let label = if remote { "remote error:" } else { "error:" };
    branch(&mut ConsoleWriter::new(&mut bytes), error, label, None)
        .expect("writing diagnostics into memory cannot fail");
    executable_tip(&mut ConsoleWriter::new(&mut bytes), error, remote, None)
        .expect("writing diagnostics into memory cannot fail");
    String::from_utf8(bytes)
        .expect("diagnostics are UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpm_exec::error::CombinedError;

    #[test]
    fn nested_aggregate_layout_keeps_linear_causes_at_each_branch_depth() {
        let inner = CombinedError::new(
            anyhow::anyhow!("Error7")
                .context("Error6")
                .context("Error5"),
            anyhow::anyhow!("Error8"),
        );
        let outer = CombinedError::new(
            anyhow::anyhow!("Error3").context("Error2"),
            anyhow::Error::new(inner).context("Error4"),
        );
        let error = anyhow::Error::new(outer)
            .context("Error1")
            .context("Error0");
        assert_eq!(
            lines(&ErrorReport::capture(&error), false).join("\n"),
            "error: Error0\n  caused by: Error1\n  caused by: Error2\n    caused by: Error3\n  and: Error4\n    caused by: Error5\n      caused by: Error6\n      caused by: Error7\n    and: Error8"
        );
    }
}
