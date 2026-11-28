//! Numbered Console launch selection using the controlling terminal.

use std::path::Path;

use anyhow::{Context, Result};
use console::Term;
use mcpm_backend::operations::server::init::Candidate;

use super::{presentation, terminal_input};
use crate::display::style::OutputStyle;

pub(super) fn select(
    candidates: &[Candidate],
    command: Option<&str>,
    invoked_path: &Path,
    replace_conf: bool,
) -> Result<Option<usize>> {
    // stdout controls interaction; input comes from the terminal, never redirected stdin.
    let term = Term::stdout();
    let style = OutputStyle::new();
    term.write_line("Select an identified server launch to initialize:")?;
    let label = if cfg!(windows) {
        "Manual init (PowerShell): "
    } else {
        "Manual init: "
    };
    let invocation = command.zip(invoked_path.to_str());
    for (index, candidate) in candidates.iter().enumerate() {
        let summary = presentation::summary(candidate);
        term.write_line(&format!(
            "{}. {}",
            index + 1,
            style.primary(&summary.identity)
        ))?;
        for detail in summary.details() {
            let line: String = detail
                .into_iter()
                .map(|(role, value)| style.role(role, value))
                .collect();
            term.write_line(&line)?;
        }
        let manual = invocation
            .and_then(|(prefix, path)| {
                presentation::manual_command(prefix, path, candidate, replace_conf)
            })
            .map_or_else(
                || style.muted("(cannot represent this candidate as a single-line command)"),
                |command| style.code(command),
            );
        term.write_line(&format!("{}{}", style.normal(label), manual))?;
        term.write_line("")?;
    }
    loop {
        term.write_str(&format!(
            "Select 1-{} (Enter/Esc/EOF cancels): ",
            candidates.len()
        ))?;
        let Some(answer) = terminal_input::read_line(&term, |ch| ch.is_ascii_digit())
            .context("failed to read server launch selection")?
        else {
            return Ok(None);
        };
        if answer.is_empty() {
            return Ok(None);
        }
        if let Ok(index) = answer.parse::<usize>()
            && (1..=candidates.len()).contains(&index)
        {
            return Ok(Some(index - 1));
        }
        term.write_line(
            &style.warning("Please enter a candidate number, or press Enter to cancel."),
        )?;
    }
}
