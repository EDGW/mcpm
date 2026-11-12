//! Console confirmations for large inventories and destructive cache removal.

use anyhow::{Context, Result};

use mcpm_rich::rich;

use crate::{
    display::{
        rich::{Choice, line, prompt},
        style::OutputStyle,
    },
    terminal,
};

pub(in crate::commands) fn reading(count: usize, noun: &str) -> Result<bool> {
    if count <= 80 || !terminal::stdout_is_tty() {
        return Ok(true);
    }
    // Read from the console rather than consuming a command's redirected stdin.
    let term = console::Term::stdout();
    let style = OutputStyle::new();
    term.write_line(&line(
        &rich!("There are [{count}](secondary) {noun}."),
        &style,
    ))?;
    term.write_line("")?;
    loop {
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
        term.write_str(&prompt(
            &rich!("Do you want to read all of them?"),
            &choices,
            &style,
        ))
        .context("failed to write list confirmation")?;
        let answer = read_answer(&term, false).context("failed to read list confirmation")?;
        term.write_line("")?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                term.write_line(&line(&rich!("[Please enter Y or n.](warning)"), &style))?;
                term.write_line("")?;
            }
        }
    }
}

/// Confirms a frozen remove-all selection without consuming the command's stdin.
pub(in crate::commands) fn cache_removal(
    path: &std::path::Path,
    target: &str,
    yes: bool,
) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    anyhow::ensure!(
        terminal::stdout_is_tty() && terminal::stderr_is_tty(),
        "cache remove --all requires confirmation; use --yes in Raw or non-interactive mode"
    );
    let term = console::Term::stderr();
    let style = OutputStyle::stderr();
    let path = path.to_string_lossy();
    let warning =
        rich!("[You are about to remove all cache entries from](warning) `{path}` on `{target}`.");
    term.write_line(&line(&warning, &style))
        .context("failed to write cache removal confirmation")?;
    term.write_line("")?;
    loop {
        let choices = [
            Choice {
                label: "y",
                default: false,
            },
            Choice {
                label: "N",
                default: true,
            },
        ];
        term.write_str(&prompt(
            &rich!("Are you sure you want to continue?"),
            &choices,
            &style,
        ))
        .context("failed to write cache removal confirmation")?;
        let answer =
            read_answer(&term, true).context("failed to read cache removal confirmation")?;
        term.write_line("")?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => {
                term.write_line(&line(&rich!("[Please enter y or N.](warning)"), &style))?;
                term.write_line("")?;
            }
        }
    }
}

#[cfg(not(windows))]
pub(in crate::commands) fn read_answer(
    term: &console::Term,
    eof_declines: bool,
) -> std::io::Result<String> {
    if !eof_declines {
        return term.read_line();
    }
    let mut answer = String::new();
    loop {
        let key = match term.read_key() {
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(String::new());
            }
            result => result?,
        };
        match key {
            console::Key::Enter => {
                term.write_line("")?;
                return Ok(answer);
            }
            console::Key::Char('\u{3}' | '\u{4}') | console::Key::Escape => {
                term.write_line("")?;
                return Ok(String::new());
            }
            console::Key::Char(ch) if !ch.is_control() => {
                answer.push(ch);
                term.write_str(&ch.to_string())?;
            }
            console::Key::Backspace if !answer.is_empty() => {
                answer.pop();
                term.clear_chars(1)?;
            }
            _ => {}
        }
    }
}

/// Crossterm reads CONIN$ on Windows even when the process stdin is a pipe.
#[cfg(windows)]
pub(in crate::commands) fn read_answer(
    term: &console::Term,
    eof_declines: bool,
) -> std::io::Result<String> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, read};

    let mut answer = String::new();
    loop {
        let Event::Key(key) = read()? else { continue };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        match key.code {
            KeyCode::Enter => {
                term.write_line("")?;
                return Ok(answer);
            }
            KeyCode::Esc => return Ok(String::new()),
            KeyCode::Char('c' | 'd' | 'z')
                if eof_declines && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                term.write_line("")?;
                return Ok(String::new());
            }
            KeyCode::Char(ch) if !ch.is_control() => {
                answer.push(ch);
                term.write_str(&ch.to_string())?;
            }
            KeyCode::Backspace if !answer.is_empty() => {
                answer.pop();
                term.clear_chars(1)?;
            }
            _ => {}
        }
    }
}
