//! Console interactive prompts for enabling or disabling RCON in `server.properties` during `init`.

use anyhow::{Context, Result};
use console::Term;
use mcpm_backend::operations::server::init::InitRconRequest;
use mcpm_lib::local::mc::server::properties::{DEFAULT_RCON_PORT, parse_rcon_port};
use mcpm_rich::rich;

use super::terminal_input;
use crate::display::{
    rich::{Choice, line, prompt as format_prompt},
    style::OutputStyle,
};

/// Prompts the user on the controlling terminal to enable (or disable incomplete) RCON settings.
pub(super) fn prompt(
    currently_enabled: bool,
    existing_port: Option<u16>,
    has_password: bool,
    invalid_port: Option<&str>,
) -> Result<Option<InitRconRequest>> {
    let term = Term::stdout();
    let style = OutputStyle::new();

    if let Some(raw_port) = invalid_port {
        let escaped = crate::display::style::text(raw_port);
        term.write_line(&line(
            &rich!(
                "[warning:](warning) Existing `rcon.port` (`{escaped}`) in `server.properties` is invalid and will be treated as unset."
            ),
            &style,
        ))?;
    }

    if currently_enabled {
        let missing = match (existing_port.is_some(), has_password) {
            (false, false) => "`rcon.port` and `rcon.password` are",
            (false, true) => "`rcon.port` is",
            (true, false) => "`rcon.password` is",
            (true, true) => unreachable!("complete RCON configurations skip prompting"),
        };
        term.write_line(&line(
            &rich!(
                "[RCON is enabled in](warning) `server.properties`[, but](warning) {missing} [not configured.](warning)"
            ),
            &style,
        ))?;
    }

    let choices = [
        Choice {
            label: "Y",
            default: true,
        },
        Choice {
            label: "n",
            default: false,
        },
    ];
    let enable = loop {
        term.write_str(&format_prompt(
            &rich!("Enable RCON in `server.properties`?"),
            &choices,
            &style,
        ))
        .context("failed to write RCON confirmation prompt")?;
        let Some(answer) = terminal_input::read_line(&term, |_| true)
            .context("failed to read RCON confirmation")?
        else {
            return Ok(None);
        };
        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => break true,
            "n" | "no" => break false,
            _ => {
                term.write_line(&line(&rich!("[Please enter Y or n.](warning)"), &style))?;
            }
        }
    };

    if !enable {
        return Ok(currently_enabled.then_some(InitRconRequest::Disable));
    }

    let port = if existing_port.is_some() {
        None
    } else {
        let default_port = DEFAULT_RCON_PORT;
        let default_label = default_port.to_string();
        let port_choices = [Choice {
            label: &default_label,
            default: true,
        }];
        loop {
            term.write_str(&format_prompt(&rich!("RCON port"), &port_choices, &style))
                .context("failed to write RCON port prompt")?;
            let Some(answer) = terminal_input::read_line(&term, |_| true)
                .context("failed to read RCON port input")?
            else {
                return Ok(None);
            };
            let trimmed = answer.trim();
            if trimmed.is_empty() {
                break Some(DEFAULT_RCON_PORT);
            }
            if let Ok(port) = parse_rcon_port(trimmed) {
                break Some(port);
            }
            term.write_line(&line(
                &rich!(
                    "[Please enter a port between 1 and 65535, or press Enter for {default_port}.](warning)"
                ),
                &style,
            ))?;
        }
    };

    Ok(Some(InitRconRequest::InteractiveEnable {
        port,
        generate_password: !has_password,
    }))
}
